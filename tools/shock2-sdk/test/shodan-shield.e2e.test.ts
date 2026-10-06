import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer, type EntityDetailResult } from "../src/index.js";

const property = (entity: EntityDetailResult, name: string) =>
  Number(entity.properties.find((entry) => entry.name === name)?.value);

// Exported so the same scenario can run against a protected prebuilt binary.
// Direct launch is a staged regression fixture, not campaign completion proof.
export async function verifyShodanShield(game: Game): Promise<void> {
  await game.step({ frames: 60 });
  const safe = (await game.info()).player.position;
  const shield = (await game.entities.byTemplate(270))[0];
  assert.ok(shield, "discover the authored shield by stable mission identity");
  const initial = await game.entities.detail(shield.id);
  assert.equal(property(initial, "HitPoints"), 112,
    "the first Regen clamps authored 115 HP to trunc(140 * 0.8)");
  assert.ok(Math.abs(property(initial, "RenderAlpha") - 0.8) < 0.00001);

  await game.player.setStats({ strength: 4, endurance: 4, agility: 3,
    skills: { standard_weapons: 6 } });
  await game.player.spawnItem(-18);
  for (let i = 0; i < 4; i++) await game.player.spawnItem("Small Standard Clip");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 2 });
  await game.input.trigger("Reload");
  await game.step({ frames: 120 });
  const [x, y, z] = shield.position;
  await game.player.teleport({ x, y: y - 1.04, z: z + 6 });
  await game.step({ frames: 1 });
  await game.player.aimAt(shield, { visibility: "required", hitbox: "torso" });
  const before = await game.entities.detail(shield.id);
  await game.input.set("right_hand.trigger_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger_value", 0);
  await game.step({ frames: 5 });
  const hit = await game.entities.detail(shield.id);
  const hitHp = property(hit, "HitPoints");
  assert.ok(hitHp > 0 && hitHp < property(before, "HitPoints"),
    "an ordinary rifle shot must damage the shield without destroying it");
  assert.ok(Math.abs(property(hit, "RenderAlpha") - hitHp / 140) < 0.00001,
    "damage updates opacity from actual applied HP before the next Regen");
  await game.player.teleport({ x: safe[0], y: safe[1], z: safe[2] });
  await game.step({ frames: 300 });
  const recovered = await game.entities.detail(shield.id);
  assert.equal(property(recovered, "HitPoints"), Math.min(hitHp + 5, 112));
  assert.ok(Math.abs(property(recovered, "RenderAlpha") - property(recovered, "HitPoints") / 140) < 0.00001);
  assert.ok((await game.info()).player.hit_points! > 0, "observer survives the recovery window");

  // Saved scripts retain their timer; entity IDs are rediscovered after load.
  await game.save("test-shodan-shield-recovery");
  await game.load("test-shodan-shield-recovery");
  const restored = (await game.entities.byTemplate(270))[0];
  assert.ok(restored);
  const savedHp = property(await game.entities.detail(restored.id), "HitPoints");
  assert.equal(savedHp, property(recovered, "HitPoints"));
  await game.step({ frames: 60 });
  assert.equal(property(await game.entities.detail(restored.id), "HitPoints"), Math.min(savedHp + 1, 112));
  await game.step({ frames: 600 });
  assert.equal(property(await game.entities.detail(restored.id), "HitPoints"), 112);
}

test("SHODAN shield restores health and opacity after a real rifle shot", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "shodan.mis" });
  await verifyShodanShield(game);
});
