import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("belt and holster calibration moves live targets independently and resolves pouch overlap", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const before = (await game.info()).player.hand_feedback!;
  await game.devParams.set("vr_belt_distance", 0.30);
  await game.step({ frames: 3 });
  const movedBelt = (await game.info()).player.hand_feedback!;
  assert.deepEqual(movedBelt.holsters!.centers, before.holsters!.centers, "belt must not move thighs");
  const delta = sub(movedBelt.ammo_pouch!.center!, before.ammo_pouch!.center!);
  assert.ok(Math.abs(Math.hypot(...delta) * 0.762 - 0.10) < 0.001, "pouch moves by ten centimetres with belt");
  await game.devParams.set("vr_holster_forward", -0.10);
  await game.step({ frames: 3 });
  const movedThighs = (await game.info()).player.hand_feedback!;
  assert.deepEqual(movedThighs.ammo_pouch!.center, movedBelt.ammo_pouch!.center, "thighs must not move pouch");
  assert.ok(Math.abs(Math.hypot(...sub(movedThighs.holsters!.centers![0], movedBelt.holsters!.centers![0])) * 0.762 - 0.14) < 0.001);

  const rack = (await game.entities.list()).entities;
  const wrench = rack.find(e => e.template_id === -928)!;
  const pistol = rack.find(e => e.template_id === -17)!;
  await aimVrHandAt(game, wrench.position, 0.2, 1);
  await game.step({ frames: 5 });
  let player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, wrench.id);
  await game.input.set("right_hand.position", quatRotate(quatConjugate(player.rotation), sub(player.hand_feedback!.holsters!.centers![0], player.position)));
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.items[0], wrench.id);
  await aimVrHandAt(game, pistol.position, 0.2, 1);
  await game.step({ frames: 5 });
  const reserve = await game.player.spawnItem(-31);
  // Put the right holster beside the pouch, with intersecting grab volumes.
  await game.devParams.set("vr_holster_drop", 0.55);
  await game.devParams.set("vr_holster_side", 0.16);
  await game.devParams.set("vr_holster_forward", 0.25);
  await game.devParams.set("vr_belt_distance", 0.20);
  await game.step({ frames: 5 });
  player = (await game.info()).player;
  const pouch = player.hand_feedback!.ammo_pouch!.center!;
  const thigh = player.hand_feedback!.holsters!.centers![0];
  const overlap = pouch.map((v, i) => (v + thigh[i]) / 2) as [number, number, number];
  await game.input.set("left_hand.position", quatRotate(quatConjugate(player.rotation), sub(overlap, player.position)));
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.ammo_pouch!.near[0], true);
  assert.equal((await game.info()).player.hand_feedback!.holsters!.near[0], null);
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.wielded_entity_id, reserve.entity_id, "one squeeze draws only ammunition");
  assert.equal((await game.info()).player.hand_feedback!.holsters!.items[0], wrench.id);
});
