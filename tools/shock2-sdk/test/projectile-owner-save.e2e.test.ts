import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

test("a saved player projectile keeps its shooter filter after entity remapping", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 30 });
  await game.player.spawnItem("Laser Pistol");
  await game.input.trigger("EquipLaserPistol");
  await game.step({ frames: 5 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);

  const shotBody = async () => {
    const shots = (await game.entities.list({ filter: "Laser Shot", limit: 50 })).entities;
    assert.equal(shots.length, 1, "the fired bolt must remain alive for the save");
    const bodies = (await game.physics.bodies({ entityId: shots[0].id })).bodies;
    assert.equal(bodies.length, 1);
    return bodies[0];
  };
  assert.equal((await shotBody()).blocks_player, false);
  assert.equal((await shotBody()).blocks_actor, true);
  const save = `projectile_owner_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  assert.equal((await shotBody()).blocks_player, false,
    "loading must not turn the player's own projectile solid to its shooter");
  assert.equal((await shotBody()).blocks_actor, true,
    "the restored projectile must still collide with enemies");
});
