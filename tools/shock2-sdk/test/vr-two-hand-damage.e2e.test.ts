import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { MeleeSwing, Vec3 } from "../src/types.js";

// A one-handed swing of a two-handed weapon lands a lesser blow; a second hand
// on the haft restores the full one. The latch is taken when the swing goes
// hot, so a hand grabbed after that cannot upgrade it.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   SHOCK2_E2E=1 npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The dev-param default; the readout is asserted against it, not a literal. */
const ONE_HAND_SCALE = 0.7;

/** Put the wrench in the right hand, and return the pawn-local hand pose it is
 * held at. Same production selection ray the grip test uses: the hand goes ON
 * the wrench, since the rack stocks several melee weapons. */
async function grabWrench(game: GameServer): Promise<[number, Vec3]> {
  const wrench = (await game.entities.list({ filter: "Wrench", limit: 20 }))
    .entities[0];
  assert.ok(wrench, "debug_melee should stock a Wrench");

  const pawn = (await game.info()).player.position;
  const onWrench: Vec3 = [
    wrench.position[0] - pawn[0],
    wrench.position[1] - pawn[1],
    wrench.position[2] - pawn[2],
  ];
  await game.input.set("right_hand.position", onWrench);
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("right_hand.squeeze", 0.0);
  // Park the off-hand well clear so nothing is supported before it is moved.
  await game.input.set("left_hand.position", [
    onWrench[0] + 2.0,
    onWrench[1],
    onWrench[2],
  ]);
  await game.input.set("left_hand.squeeze", 0.0);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 1.0);
  await game.step({ frames: 8 });

  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    wrench.id,
    "the right hand should hold the Wrench",
  );
  return [wrench.id, onWrench];
}

/** Close the off-hand on the weapon's shaft (above the grip - the melee wield
 * parks the contact body on the weapon head) and let the attach ramp finish. */
async function supportAt(game: GameServer, at: Vec3): Promise<void> {
  await game.input.set("left_hand.position", at);
  await game.input.set("left_hand.squeeze", 0.0);
  await game.step({ frames: 4 });
  await game.input.set("left_hand.squeeze", 1.0);
  await game.step({ frames: 20 });
}

/**
 * Swing the held weapon by sliding the tracked hand (and, when it is holding
 * on, the support hand with it) along +Z. Returns EVERY frame the swing spent
 * hot, so a caller asserts over the whole swing rather than over one sample it
 * chose - a latch that wrongly flips mid-swing must not be able to hide in an
 * unread frame.
 *
 * `grabDuring` closes the off-hand on the shaft partway through the sweep -
 * the late grab the latch is meant to refuse.
 */
async function swing(
  game: GameServer,
  from: Vec3,
  options: { support?: Vec3; grabDuring?: Vec3 } = {},
): Promise<MeleeSwing[]> {
  const FRAMES = 30;
  const TRAVEL = 3.0;
  const hot: MeleeSwing[] = [];
  for (let frame = 1; frame <= FRAMES; frame += 1) {
    const dz = (TRAVEL * frame) / FRAMES;
    await game.input.set("right_hand.position", [from[0], from[1], from[2] + dz]);
    if (options.support) {
      await game.input.set("left_hand.position", [
        options.support[0],
        options.support[1],
        options.support[2] + dz,
      ]);
    }
    if (options.grabDuring && frame === Math.round(FRAMES / 2)) {
      // Mid-swing: close the off-hand where the shaft has travelled to. Set
      // BEFORE the step, so the swing never spends a frame standing still -
      // a stationary frame drops below the gate and legitimately ends the
      // swing, and the next one would be a fresh (two-handed) swing.
      await game.input.set("left_hand.position", [
        options.grabDuring[0],
        options.grabDuring[1],
        options.grabDuring[2] + dz,
      ]);
      await game.input.set("left_hand.squeeze", 1.0);
    }
    await game.step({ frames: 1 });
    const swing = (await game.info()).player.melee.swing;
    if (swing.hot) hot.push(swing);
  }
  assert.ok(hot.length > 0, "the sweep should cross the swing gate");
  return hot;
}

/** Assert every hot frame of one swing agrees on what it was worth. */
function assertSwingWorth(
  swing: MeleeSwing[],
  latched: boolean,
  scale: number,
  what: string,
): void {
  for (const [frame, sample] of swing.entries()) {
    assert.equal(
      sample.two_handed_latched,
      latched,
      `${what}: frame ${frame} of ${swing.length} read latched=${sample.two_handed_latched}`,
    );
    assert.ok(
      Math.abs(sample.damage_scale - scale) < 1e-3,
      `${what}: frame ${frame} should be worth ${scale}, got ${sample.damage_scale}`,
    );
  }
}

test(
  "VR: a one-handed swing is worth less than the same swing in two hands",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 30 });

    const [, hand] = await grabWrench(game);
    // The wrench's fitted contact volume runs up out of the tracked hand, so
    // the shaft a second hand can take is above the grip.
    const shaft: Vec3 = [hand[0], hand[1] + 0.6, hand[2]];

    // A weapon at rest is not swinging anything.
    const idle = (await game.info()).player.melee.swing;
    assert.equal(idle.hot, false, "a still weapon is not mid-swing");

    // --- one hand ---
    const oneHanded = await swing(game, hand);
    assertSwingWorth(oneHanded, false, ONE_HAND_SCALE, "a one-handed wrench swing");

    // --- two hands ---
    await game.input.set("right_hand.position", hand);
    await game.step({ frames: 30 });
    await supportAt(game, shaft);
    assert.equal(
      (await game.info()).player.two_handed,
      true,
      "the off-hand should have taken hold of the shaft",
    );
    const twoHanded = await swing(game, hand, { support: shaft });
    assertSwingWorth(twoHanded, true, 1.0, "a two-handed wrench swing");

    // --- a hand grabbed after the swing went hot ---
    await game.input.set("left_hand.squeeze", 0.0);
    await game.input.set("right_hand.position", hand);
    await game.input.set("left_hand.position", [hand[0] + 2.0, hand[1], hand[2]]);
    await game.step({ frames: 30 });
    assert.equal(
      (await game.info()).player.two_handed,
      false,
      "the off-hand should be clear again before the late-grab swing",
    );
    // Asserted over EVERY hot frame, so a wrong upgrade after the grab lands
    // cannot hide in a frame the test did not read.
    const lateGrab = await swing(game, hand, { grabDuring: shaft });
    assertSwingWorth(lateGrab, false, ONE_HAND_SCALE, "a swing grabbed after it went hot");
    // ...and the grab really did take hold - otherwise the assertion above
    // would pass for the wrong reason.
    assert.equal(
      (await game.info()).player.two_handed,
      true,
      "the late grab should still have attached the support hand",
    );
  },
);
