import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary, Vec3 } from "../src/index.js";
import { quatConjugate, quatRotate, sub, type Quat } from "./helpers/vr-hand.js";

// The player's body anchors: shoulders that stand in for the backpack (release
// what you hold there to stow it, grip an empty hand there to draw the last
// weapon stowed) and a belt card that runs a reader's credential check.
//
// Everything here is driven through `player.body_frame`, which reports where
// the anchors are this frame - so the tests place hands at the reported
// anchors rather than at guessed offsets.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** A world point in the hand channel's pawn-local space. */
function toPawnLocal(world: Vec3, pawnPosition: Vec3, pawnRotation: Quat): Vec3 {
  return quatRotate(quatConjugate(pawnRotation), sub(world, pawnPosition));
}

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

test(
  "VR: a shoulder stows what the hand holds, and draws it back",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    const before = await game.info();
    const frame = before.player.body_frame;
    assert.ok(frame, "VR should report the player's body anchors");
    assert.ok(
      frame.belt[1] < frame.left_shoulder[1],
      `the belt (${frame.belt[1]}) should sit below the shoulders (${frame.left_shoulder[1]})`,
    );
    assert.equal(frame.last_stowed_weapon, null, "nothing is stowed at spawn");

    const [wrench] = (await game.entities.list({ filter: "Wrench" })).entities;
    assert.ok(wrench, "debug_melee racks a wrench");
    // Counted with both hands empty: a held item already counts as carried, so
    // measuring after the grab would see no growth when it is stowed.
    const carried = (await game.player.inventory()).items.length;

    // Reach for the wrench: the hand sits short of it and aims at it (45 deg
    // about Y takes the hand's -Z forward onto the rack).
    const pawn = before.player.position as Vec3;
    await game.input.set("right_hand.position", [
      wrench.position[0] - pawn[0] + 0.2,
      wrench.position[1] - pawn[1],
      wrench.position[2] - pawn[2] + 0.2,
    ]);
    await game.input.set("right_hand.rotation", [0, 0.38268, 0, 0.92388]);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "the racked wrench should light the glove",
    );

    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.hand_affordance.right_model,
      "wrench_h",
      "the squeeze should take the wrench into the hand",
    );

    // Carry it to the shoulder. The zone pre-lights before the release.
    const shoulder = toPawnLocal(
      frame.right_shoulder as Vec3,
      pawn,
      before.player.rotation as Quat,
    );
    await game.input.set("right_hand.position", shoulder);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "a full hand at the shoulder should show that opening it will stow",
    );

    // Open the hand: the wrench goes in the backpack, not on the floor.
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 5 });
    const stowed = await game.info();
    assert.equal(
      stowed.player.hand_affordance.right_model,
      null,
      "the hand should be empty after the stow",
    );
    assert.equal(
      stowed.player.body_frame?.last_stowed_weapon,
      "Wrench",
      "the shoulder should remember what it took",
    );
    assert.equal(
      (await game.player.inventory()).items.length,
      carried + 1,
      "the stowed wrench should be in the backpack",
    );
    assert.ok(
      (await game.player.inventory()).items.some((item) => item.name === "Wrench"),
      "and it should be the wrench that landed there",
    );

    // Grip the empty hand there: the same weapon comes back out.
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.hand_affordance.right_model,
      "wrench_h",
      "an empty grip at the shoulder should draw the last weapon stowed",
    );
  },
);

test(
  "VR: the belt card opens a reader the collected credential opens",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    // The Cryo Card's own reader: MedSci authors six card slots, and this is
    // the one whose key destination the Cryo Card satisfies. Picked by
    // authored position (stable mission data) - runtime entity ids are not.
    const CRYO_SLOT: Vec3 = [-26.93811, 0.16234326, -5.46197];
    const distance = (e: EntitySummary) =>
      Math.hypot(...CRYO_SLOT.map((c: number, i: number) => c - e.position[i]));
    const slots = (await game.entities.list({ filter: "Card slot" })).entities;
    const slot = slots.reduce((a, b) => (distance(a) <= distance(b) ? a : b));
    assert.ok(distance(slot) < 0.5, "the Cryo Card's own slot should be found");
    const slotModel = async () =>
      property(await game.entities.detail(slot.id), "Model");
    assert.equal(await slotModel(), "cardslor", "the slot starts locked (red)");

    assert.equal(
      (await game.info()).player.body_frame?.belt_card,
      null,
      "no credential collected yet, so there is no belt card",
    );

    // Stand at the slot, hand held against it. With no card, nothing opens.
    await game.player.teleport({
      x: CRYO_SLOT[0],
      y: CRYO_SLOT[1] + 0.5,
      z: CRYO_SLOT[2] + 1.2,
    });
    await game.step({ frames: 10 });
    const staged = await game.info();
    const pawn = staged.player.position as Vec3;
    const pawnRotation = staged.player.rotation as Quat;
    const atSlot = toPawnLocal(
      [CRYO_SLOT[0], CRYO_SLOT[1], CRYO_SLOT[2] + 0.15],
      pawn,
      pawnRotation,
    );
    await game.input.set("right_hand.position", atSlot);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    assert.equal(
      await slotModel(),
      "cardslor",
      "an empty hand at the reader must not open it",
    );
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Blocked",
      "a lock the player has no credential for should read blocked",
    );

    // Collect the credential the way frobbing the card does. The card itself
    // is never inventoried - the belt card is what appears.
    const [card] = (await game.entities.list({ filter: "Cryo Card" })).entities;
    assert.ok(card, "medsci1 places the Cryo Card");
    await game.entities.sendMessage(card.id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.body_frame?.belt_card,
      "belt",
      "collecting a credential should put the card on the belt",
    );

    // Take it off the hip.
    const onBelt = await game.info();
    const belt = toPawnLocal(
      onBelt.player.body_frame!.belt as Vec3,
      onBelt.player.position as Vec3,
      pawnRotation,
    );
    await game.input.set("right_hand.position", belt);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "the belt should offer the card to an empty hand",
    );
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.body_frame?.belt_card, "right");

    // Hold it against the reader: the same credential check a frob runs.
    await game.input.set("right_hand.position", atSlot);
    await game.step({ frames: 10 });
    assert.equal(
      await slotModel(),
      "cardslog",
      "the card should unlock the reader it has the credential for",
    );

    // Opening the hand puts the card back on the belt - it is never dropped.
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.body_frame?.belt_card, "belt");
  },
);
