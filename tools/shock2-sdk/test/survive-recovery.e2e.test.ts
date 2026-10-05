import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("Survive recovers the same living attacker after the player changes floors", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde" });
  await game.devParams.set("cheat", 1);
  await game.devParams.set("horde_tech_enabled", 0);
  await game.step({ frames: 2 });
  await game.player.teleport({ x: 10, y: 1.8, z: 7 });
  await game.step({ frames: 5 });
  const [director] = (await game.entities.list({ filter: "Containment - Ready" })).entities;
  assert.ok(director);
  await game.entities.sendMessage(director.id, { type: "Frob" });
  await game.step({ frames: 5 });
  const markers = (await game.entities.list({ filter: "Containment spawn" })).entities;
  const spawned = (await Promise.all(markers.map(m => game.entities.detail(m.id))))
    .flatMap(m => m.outgoing_links).find(link => link.link_type === "Spawned");
  assert.ok(spawned, "first attacker spawned downstairs");
  const attacker = await game.entities.detail(spawned.target_id);
  assert.ok(attacker.position[1] < 10);
  await game.player.teleport({ x: 11.6, y: 23.36, z: 42 });
  await game.step({ frames: 120 });
  assert.ok((await game.entities.detail(attacker.entity_id)).position[1] < 10,
    "a floor change must not immediately teleport an attacker");
  let rescued = false;
  for (let i = 0; i < 90; i++) {
    await game.step({ frames: 60 });
    const current = await game.entities.detail(attacker.entity_id);
    if (current.position[1] > 10) {
      rescued = true;
      assert.notEqual(current.properties.find(p => p.name === "AIBehavior")?.value, "Dead");
      break;
    }
  }
  assert.ok(rescued, "the stranded attacker must return to the player's floor without being killed/replaced");
});
