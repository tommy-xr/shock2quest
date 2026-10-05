import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type EntitySummary } from "../src/index.js";

test("Survive pyrotechnic monkey launches its inactive projectile template", {
  skip: process.env.SHOCK2_E2E !== "1",
  timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde" });
  await game.devParams.set("horde_start_wave", 6);
  await game.devParams.set("cheat", 1);
  await game.player.teleport({ x: 0, y: 21.044, z: 24 });
  await game.step({ frames: 3600 });
  const cleared = new Set<number>();
  let monkey: EntitySummary | undefined;
  // Wave 6 guarantees a red monkey. Clear earlier spawns at their distant
  // spawn sites, before packmates can block the monkey's line of fire.
  for (let frame = 0; frame < 3000 && !monkey; frame += 30) {
    const { entities } = await game.entities.list();
    monkey = entities.find((entity) => entity.template_id === -1432);
    for (const entity of entities) {
      if (entity.template_id >= 0 || entity.script_count === 0 ||
          entity.id === monkey?.id || cleared.has(entity.id)) continue;
      const detail = await game.entities.detail(entity.id);
      if (detail.properties.some((property) => property.name === "AIBehavior")) {
        await game.entities.sendMessage(entity.id, { type: "Damage", amount: 10000 });
        cleared.add(entity.id);
      }
    }
    if (!monkey) await game.step({ frames: 30 });
  }
  assert.ok(monkey, "wave 6 must spawn its guaranteed red monkey");
  // Both street and landing are authored spawn areas. Stay on their surveyed
  // floors and 8–12 units away; closer monkeys choose their melee attack.
  const [x, , z] = monkey.position;
  await game.player.teleport(z > 35
    ? { x: 11.6, y: 23.644, z: z >= 50 ? 44 : 56 }
    : { x: x > 10 ? x - 8 : x + 8, y: 21.044, z });
  await game.entities.sendMessage(monkey.id, { type: "SetAlertness", level: "High" });
  let shot: EntitySummary | undefined;
  for (let frame = 0; frame < 600 && !shot; frame++) {
    await game.step({ frames: 1 });
    [shot] = await game.entities.byTemplate(-2032);
  }
  assert.ok(shot, "the monkey must emit a real Red Monkey Shot");
  assert.equal((await game.entities.detail(shot.id)).has_refs, true,
    "launch must activate the authored HasRefs=false projectile");
  const { bodies } = await game.physics.bodies({ entityId: shot.id });
  assert.equal(bodies.length, 1, "the emitted shot must have a physics body");
  await game.step({ frames: 3 });
  const moved = await game.entities.detail(shot.id);
  assert.ok(Math.hypot(...moved.position.map((value, axis) => value - shot!.position[axis]!)) > 0.3,
    "the fireball must leave the monkey's muzzle");
  await game.step({ frames: 90 });
  assert.ok(!(await game.entities.byTemplate(-2032)).some((entity) => entity.id === shot!.id),
    "the observed fireball must impact and be removed");
});
