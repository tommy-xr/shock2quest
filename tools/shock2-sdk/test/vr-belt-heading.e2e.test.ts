import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { setHandWorldPose } from "../src/vr-pose.js";
import { add, aimVrHandAt, reachHolster, sub } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

/** Belt heading in degrees, read off the two holster centres (slot 0 is right of slot 1). */
async function beltHeading(game: GameServer) {
  const centers = (await game.info()).player.hand_feedback?.holsters?.centers;
  assert.ok(centers, "tracked holster zones must be published");
  const right = sub(centers[0], centers[1]);
  return (Math.atan2(right[2], right[0]) * 180) / Math.PI;
}

/** Unsigned belt turn since `from`, after one second of a 90 degree head turn. */
async function turnHead(game: GameServer, from: number, yaw: number) {
  await game.input.set("head.look", [yaw, 0]);
  await game.step({ frames: 60 });
  const delta = (await beltHeading(game)) - from;
  return Math.abs(Math.atan2(Math.sin((delta * Math.PI) / 180), Math.cos((delta * Math.PI) / 180)) * 180 / Math.PI);
}

/** Face along head.look yaw 0 with the rack pistol in the right hand. */
async function grabPistol(game: GameServer) {
  await game.input.set("head.look", [0, 0]);
  await game.step({ frames: 60 });
  const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17)!;
  assert.ok(pistol);
  await aimVrHandAt(game, pistol.position, 0.2, 1);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.right_hand_entity_id, pistol.id);
  return pistol;
}

test("a hand resting at a holster does not stop the belt following the head", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  const pistol = await grabPistol(game);
  await reachHolster(game, "right", 0);
  // Squeezing at the slot is a grab: the slot must not swing out from under the hand.
  const start = await beltHeading(game);
  assert.ok((await turnHead(game, start, 90)) < 1, "a grab at the holster pins the heading");
  await game.input.set("head.look", [0, 0]);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  const player = (await game.info()).player;
  assert.deepEqual(player.hand_feedback?.holsters?.items, [pistol.id, null]);
  assert.equal(player.hand_feedback?.holsters?.near[1], 0, "the open hand still rests at the holster");
  const rest = await beltHeading(game);
  const turned = await turnHead(game, rest, 90);
  assert.ok(Math.abs(turned - 90) < 10, `belt follows the head within a second (turned ${turned.toFixed(1)} deg)`);
});

test("an open hand hanging at the ammo pouch does not stop the belt following the head", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await grabPistol(game);
  await game.input.set("right_hand.position", [0.3, 1, -0.4]);
  await game.step({ frames: 3 });
  let player = (await game.info()).player;
  const center = player.hand_feedback?.ammo_pouch?.center;
  assert.ok(center);
  // Storage zones use the calibrated palm, not the controller origin.
  await setHandWorldPose(game, player, "left", center, player.rotation);
  await game.step({ frames: 3 });
  player = (await game.info()).player;
  const palm = player.hand_feedback?.glove_contacts?.centers[0];
  assert.ok(palm);
  await setHandWorldPose(game, player, "left", add(center, sub(center, palm)), player.rotation);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback?.ammo_pouch?.near[0], true);
  const start = await beltHeading(game);
  const turned = await turnHead(game, start, 90);
  assert.ok(Math.abs(turned - 90) < 10, `belt follows the head within a second (turned ${turned.toFixed(1)} deg)`);
});
