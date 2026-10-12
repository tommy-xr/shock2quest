import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { Game, GameServer, type EntityDetailResult, type Vec3 } from "../src/index.js";

const ring = [257, 261, 263, 265, 267, 269, 271, 273];
const shields = [270, 272, 274, 275, 277, 278, 279, 280];
const close = (a: readonly number[], b: readonly number[]) =>
  a.every((v, i) => Math.abs(v - b[i]) < 0.0001);

// Also used with immutable prebuilt base/fix executables by the PR capture.
export async function verifyShodanShieldOrbit(game: Game): Promise<void> {
  // Stage a stable observer on the arena floor; the authored start is a long slide.
  await game.player.teleport({ x: 32, y: -90.3, z: 83 });
  const detail = async (template: number): Promise<EntityDetailResult> => {
    const entity = (await game.entities.byTemplate(template))[0];
    assert.ok(entity, `authored object ${template} exists`);
    return game.entities.detail(entity.id);
  };
  const nodes = await Promise.all(ring.map(detail));
  const initial = await Promise.all(shields.map(detail));
  const starts = initial.map(s => nodes.findIndex(n => close(n.position, s.position)));
  assert.ok(starts.every(i => i >= 0));
  const check = async (hop: number, alive = shields) => {
    for (const template of alive) {
      const actual = await detail(template);
      const expected = nodes[(starts[shields.indexOf(template)] + hop) % ring.length];
      assert.ok(close(actual.position, expected.position), `${template} at ring hop ${hop}: ${actual.position} vs ${expected.position}`);
      // Quaternions q and -q encode the same facing.
      assert.ok(close(actual.rotation, expected.rotation) || close(actual.rotation, expected.rotation.map(v => -v)), `${template} copies marker facing`);
    }
  };

  await game.step({ frames: 89 });
  for (let i = 0; i < shields.length; i++) {
    assert.ok(close((await detail(shields[i])).position, initial[i].position));
  }
  await game.step({ frames: 1 }); // 1.5 s: copy the initial marker, then advance cursor.
  await check(0);
  await game.step({ frames: 89 });
  await check(0);
  // Teleport effects reach the physics body at 3.0 s; the shared physics
  // pose sync publishes PropPosition on the next frame.
  await game.step({ frames: 2 }); // first visible hop; negative on main.
  await check(1);

  // Destroy one membrane through its real damage scripts; surviving shields
  // continue moving and leave a different unoccupied ring node every tick.
  await game.entities.sendMessage((await detail(270)).entity_id, { type: "Damage", amount: 999 });
  await game.step({ frames: 1 });
  assert.equal((await game.entities.byTemplate(270)).length, 0);
  const alive = shields.filter(id => id !== 270);
  await game.step({ frames: 43 });
  const assetRoot = process.env.DARK_ASSET_PATH;
  assert.ok(assetRoot, "set the verified asset root explicitly");
  const saveName = `test-shodan-orbit-${randomUUID()}`;
  try {
    await game.save(saveName);
    await game.load(saveName);
    await check(1, alive);
    assert.equal((await game.entities.byTemplate(270)).length, 0, "destroyed shield remains absent after load");
    await game.step({ frames: 44 });
    await check(1, alive);
    await game.step({ frames: 2 });
    await check(2, alive);
    const gap = nodes[(starts[0] + 2) % ring.length].position;
    const occupied: Vec3[] = (await Promise.all(alive.map(detail))).map(s => s.position);
    assert.ok(!occupied.some(p => close(p, gap)), "destroyed shield's gap rotated with its peers");
    for (let hop = 3; hop <= 8; hop++) {
      await game.step({ frames: 90 });
      await check(hop, alive);
    }
    assert.ok((await game.info()).player.hit_points! > 0);
  } finally {
    rmSync(join(assetRoot, "saves", `${saveName}.sav`), { force: true });
  }
}

for (const vr of [false, true]) {
  test(`SHODAN TelePath shields orbit, retain facing and resume saved gaps (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "shodan.mis", debugFlags: vr ? ["--vr"] : [] });
    await verifyShodanShieldOrbit(game);
  });
}
