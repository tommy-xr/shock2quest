import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer } from "../src/index.js";

// The shield models use MD_MAT_COLOR with solid quads and no UV indices.
// Discover live IDs: mission IDs are stable, runtime IDs change after every load.
export async function verifyShodanShieldVisibility(game: Game): Promise<void> {
  await game.step({ frames: 60 });
  await game.player.teleport({ x: 32, y: -89.04, z: 83 });
  await game.camera.set({ position: [32, -88, 83], lookAt: [32, -88, 72] });
  await game.step({ frames: 1 });
  const shields = await Promise.all([270, 272, 274, 275, 277, 278, 279, 280]
    .map(async (template) => {
      const entity = (await game.entities.byTemplate(template))[0];
      assert.ok(entity, `authored shield ${template} exists`);
      return entity;
    }));
  const scene = await game.scene.objects({ limit: 10000 });
  for (const shield of shields) {
    const draws = scene.objects.filter((object) => object.entity_id === shield.id);
    assert.ok(draws.length > 0,
      `${shield.name} must submit its solid-color geometry, even without UVs or a texture asset`);
    assert.ok(draws.every((object) => object.model?.toLowerCase().startsWith("s_shiel")));
  }
}

for (const vr of [false, true]) {
  test(`SHODAN solid-color shields render in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis", debugFlags: vr ? ["--vr"] : [],
    });
    await verifyShodanShieldVisibility(game);
  });
}
