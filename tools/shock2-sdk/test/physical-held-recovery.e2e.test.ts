import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { ammoOf } from "./helpers/weapon.js";

test("a snagged physical gun returns only after its destination and arm corridor clear", {
  skip: process.env.SHOCK2_E2E !== "1",
  timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({
    mission: "debug_interactions",
    debugFlags: ["--vr", "--experimental", "physical_held_items"],
  });
  await game.step({ frames: 60 });
  const gun = (await game.entities.list({ limit: 300 })).entities.find(e => e.template_id === -18);
  assert.ok(gun?.position, "the interaction rack must provide an assault rifle");
  await game.player.teleport({ x: gun.position[0], y: 1.244, z: 0 });
  await aimVrHandAt(game, gun.position, 0.3, 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
  await game.input.set("right_hand.position", [0, 1, 0]);
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("head.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 60 });
  // Setup only: once the snag starts, all pawn motion uses the real stick.
  await game.player.teleport({ x: 0, y: 1.244, z: 0 });
  await game.step({ frames: 10 });
  const body = async () => {
    const bodies = (await game.physics.bodies({ entityId: gun.id })).bodies;
    assert.equal(bodies.length, 1);
    assert.equal(bodies[0]!.body_type, "kinematic");
    return bodies[0]!;
  };
  const ammoBefore = ammoOf(await game.entities.detail(gun.id));
  const conditionBefore = (await game.info()).player.wielded_gun_condition;
  const clearPawn = (await game.info()).player.position;
  const clearBody = (await body()).position;
  const controllerDelta = [0.3, -1.4, -0.5];
  const clearRelative = clearBody.map((v, i) => v - clearPawn[i]! + controllerDelta[i]!);

  // The waist-high button post is centered at (1.2, 0.6, -1.4).
  // Push the muzzle into it, then leave the tracked hand beyond the post.
  await game.input.set("right_hand.position", [1.2, -0.6, -0.6]);
  await game.step({ frames: 60 });
  for (let i = 0; i < 12; i++) {
    await game.input.set("right_hand.position", [1.2, -0.6, -0.6 - i * 0.1]);
    await game.step({ frames: 3 });
  }
  await game.step({ frames: 120 });
  const blocked = (await body()).position;
  assert.ok(blocked[2] > -1, "an obstructed arm corridor must not summon the gun through the post");
  const beforeWalk = (await game.info()).player.position;
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 24 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.input.set("right_hand.position", [0.3, -0.4, -0.5]);
  await game.step({ frames: 20 });
  const afterWalk = (await game.info()).player;
  assert.ok(afterWalk.position[2] < beforeWalk[2] - 3, "the pawn must walk around the finite obstacle");
  const recovered = (await body()).position;
  const error = Math.hypot(...recovered.map((v, i) => v - afterWalk.position[i]! - clearRelative[i]!));
  assert.ok(error < 0.1, `the clear destination must recover the snagged gun (remaining error ${error})`);
  assert.equal(afterWalk.right_hand_entity_id, gun.id, "recovery preserves the held entity");
  assert.equal(ammoOf(await game.entities.detail(gun.id)), ammoBefore);
  assert.equal(afterWalk.wielded_gun_condition, conditionBefore);
});
