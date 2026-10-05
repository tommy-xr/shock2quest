import assert from "node:assert/strict";
import type { GameServer } from "../../src/index.js";
import { setHandWorldPose } from "../../src/vr-pose.js";
import { add, quatConjugate, quatRotate, sub } from "./vr-hand.js";

/** Draw the real offered ammo entity, with a fresh squeeze at the belt pouch. */
export async function drawPouchAmmo(game: GameServer, hand: "left" | "right") {
  const player = (await game.info()).player;
  const pouch = player.hand_feedback?.ammo_pouch;
  assert.ok(pouch?.center);
  const offer = pouch.offers[hand === "left" ? 0 : 1];
  assert.ok(offer);
  await game.input.set(`${hand}_hand.position`, quatRotate(quatConjugate(player.rotation), sub(pouch.center, player.position)));
  await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 2 });
  // Storage zones use the calibrated palm, not the controller origin.
  const reached = (await game.info()).player;
  const palm = reached.hand_feedback?.glove_contacts?.centers[hand === "left" ? 0 : 1];
  const center = reached.hand_feedback?.ammo_pouch?.center;
  assert.ok(palm && center);
  await setHandWorldPose(game, reached, hand, add(pouch.center, sub(center, palm)), reached.rotation);
  await game.step({ frames: 3 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 8 });
  const held = (await game.info()).player;
  const entityId = hand === "left" ? held.wielded_entity_id : held.right_hand_entity_id;
  assert.ok(entityId != null, "pouch draw must put the offered ammo in the hand");
  return { offer, player: held, entityId };
}
