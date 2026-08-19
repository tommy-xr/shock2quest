import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, lookQuat } from "../src/index.js";
import type { EntitySummary, Quat, Vec3 } from "../src/types.js";
import {
  add,
  normalize,
  quatConjugate,
  quatMultiply,
  quatNormalize,
  quatRotate,
  sub,
} from "./helpers/vr-hand.js";

// Why does a VR melee swing that the e2e suite measures at a reliable 9 HP not
// land for a player in a headset?
//
// `medsci-saved-vr-melee.e2e.test.ts` and `vr-melee-contact.e2e.test.ts` both
// arm the trigger several units away from the victim and then bring the weapon
// in. That gesture always produces a fresh Rapier `CollisionStarted`, which is
// the only thing `TriggeredMeleeWeapon` damages on. A player holding a weapon
// in a corridor does not reliably reproduce it.
//
// This test pins the difference down against one shipped Blue Monkey, with the
// same weapon and the same trigger, changing only WHEN contact begins:
//
//   1. contact that begins BEFORE the trigger pull -> `Collided` is delivered,
//      and no damage ever follows (#1048);
//   2. contact that begins AFTER it, from a continuous one-frame-per-sample
//      swing -> exactly one authored 9-HP hit.
//
// It also records the contact volume's measured size, because that is the
// other half of why (1) bites in practice: the volume is the loose-prop pickup
// box, a 1.83-unit slab, so in a real room it is touching something most of
// the time (#1049).
//
// Assertions marked TODO encode today's *broken* behavior deliberately, so the
// suite stays green and CI is not broken by a known bug. They are written to
// fail loudly once the bug is fixed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const WRENCH_ARCHETYPE = -928;
const MONKEY = 543;

async function byMissionId(
  game: GameServer,
  name: string,
  missionId: number,
): Promise<EntitySummary> {
  const entity = (
    await game.entities.list({ filter: name, limit: 30 })
  ).entities.find((candidate) => candidate.template_id === missionId);
  assert.ok(entity, `expected ${name} mission object ${missionId}`);
  return entity;
}

async function hitPoints(game: GameServer, id: number): Promise<number> {
  const detail = await game.entities.detail(id);
  const property = detail.properties.find((p) => p.name === "HitPoints");
  assert.ok(property, `entity ${id} should expose HitPoints`);
  return Number(property.value);
}

/** Payload names delivered to `targetId` since `sequence`, with the other
 *  party each one names (the sender, or for `Collided` the thing touched). */
async function messagesTo(
  game: GameServer,
  sequence: number,
  targetId: number,
): Promise<{ payload: string; fromId: number | null }[]> {
  return (await game.messages.recent()).messages
    .filter((m) => m.sequence > sequence && m.to.entity_id === targetId)
    .map((m) => ({ payload: m.payload, fromId: m.from?.entity_id ?? null }));
}

const payloadNames = (
  messages: { payload: string; fromId: number | null }[],
): string[] => messages.map((m) => m.payload);

const lastSequence = async (game: GameServer): Promise<number> =>
  (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;

test(
  "VR melee damages only on contact that BEGINS inside the trigger window",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT_SWING ?? 8141),
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const wrench = await game.player.spawnItem(WRENCH_ARCHETYPE);
    await game.input.set("right_hand.squeeze", 1);
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.entity_id,
      "the spawned Wrench should be wielded",
    );

    // Measure the contact volume live: its hand-local offset (the gesture
    // below has to place the *volume*, not the hand) and its size.
    await game.input.set("right_hand.position", [0, 1.0, 0]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    // Let the pawn finish falling first, or the hand trails the body the
    // reading is compared against.
    await game.step({ frames: 45 });
    const wrenchBodies = await game.physics.bodies({
      entityId: wrench.entity_id,
    });
    const wrenchBody = wrenchBodies.bodies[0];
    assert.ok(wrenchBody, "the held Wrench must have a contact body");
    const pawnRotation = (await game.info()).player.rotation as Quat;
    const handWorld = add(
      wrenchBodies.player_position,
      quatRotate(pawnRotation, [0, 1.0, 0]),
    );
    const contactOffset = quatRotate(
      quatConjugate(pawnRotation),
      sub(wrenchBody.position, handWorld),
    );

    const monkey = await byMissionId(game, "Blue Monkey", MONKEY);
    const monkeyBody = (await game.physics.bodies({ entityId: monkey.id }))
      .bodies[0];
    assert.ok(monkeyBody, "the Blue Monkey must have an actor body");

    // TODO(#1049): the held weapon keeps the loose-prop pickup box it had on
    // the floor - `set_held_melee` never reshapes it - so its damage volume is
    // as long as the creature it hits is tall. Asserted rather than merely
    // noted, because it is the reason a real swing is so often already in
    // contact before the trigger (#1048). When the wield installs a
    // weapon-sized shape this becomes the failing signal to retune the staging
    // below and drop this block.
    assert.equal(wrenchBody.shape, "cuboid");
    const longestWrenchExtent = Math.max(...(wrenchBody.shape_extents ?? [0]));
    assert.ok(
      longestWrenchExtent > 1.5,
      `known-bug baseline (#1049): the Wrench's contact volume is the world model's bounding box: ${JSON.stringify(
        wrenchBody.shape_extents,
      )} vs the victim's ${JSON.stringify(monkeyBody.shape_extents)}`,
    );

    // Stage square in front of the monkey, and hold it still so the contact
    // geometry is about the swing rather than about the AI walking away.
    const live = await game.entities.detail(monkey.id);
    await game.player.teleport({
      x: live.position[0] + 0.815,
      y: live.position[1] + 0.236,
      z: live.position[2] - 2.245,
    });
    await game.entities.sendMessage(monkey.id, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 5 });

    /**
     * The live torso aim point and the hand frame that reaches it. Both must
     * be re-read before each phase: a nudged or startled actor drifts, and a
     * gesture staged against a stale torso swings through empty air - which
     * looks exactly like the bug under test.
     */
    async function aimAtTorso(): Promise<{
      torso: Vec3;
      placeVolumeAt: (volumeWorld: Vec3) => Promise<void>;
    }> {
      const staged = await game.entities.detail(monkey.id);
      const torso =
        (staged.aim_points?.find((p) => p.classification === "torso")
          ?.position as Vec3) ?? (staged.position as Vec3);
      const info = await game.info();
      const pawn = info.player.position as Vec3;
      const pawnQ = info.player.rotation as Quat;
      const eye = add(pawn, [0, info.player.camera_offset[1], 0]);
      const weaponQ = lookQuat(normalize(sub(torso, eye)));
      await game.input.lookAtWorldPoint(torso, {
        eyeHeight: info.player.camera_offset[1],
      });
      return {
        torso,
        // Place the *contact volume* (not the hand) at a world position.
        placeVolumeAt: async (volumeWorld: Vec3) => {
          const worldHand = sub(volumeWorld, quatRotate(weaponQ, contactOffset));
          await game.input.set(
            "right_hand.position",
            quatRotate(quatConjugate(pawnQ), sub(worldHand, pawn)),
          );
          await game.input.set(
            "right_hand.rotation",
            quatNormalize(quatMultiply(quatConjugate(pawnQ), weaponQ)),
          );
        },
      };
    }

    const resting = await aimAtTorso();

    // --- 1. Contact that begins BEFORE the trigger. ---
    await game.input.set("right_hand.trigger", 0);
    const restingSequence = await lastSequence(game);
    await resting.placeVolumeAt(resting.torso);
    await game.step({ frames: 30 });
    const restingMessages = await messagesTo(game, restingSequence, monkey.id);
    assert.ok(
      restingMessages.some(
        (m) => m.payload === "Collided" && m.fromId === wrench.entity_id,
      ),
      // Specifically a contact with the WRENCH: the monkey touches the floor
      // and its neighbours constantly, so a bare "some Collided arrived" would
      // pass even if the weapon never reached it - and this assertion is the
      // whole basis for calling the next one an edge-semantics bug rather than
      // a miss.
      `the weapon's volume must genuinely be touching the victim: ${JSON.stringify(restingMessages)}`,
    );

    const beforeHold = await hitPoints(game, monkey.id);
    const holdSequence = await lastSequence(game);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 30 });
    const held = payloadNames(await messagesTo(game, holdSequence, monkey.id));
    // TODO(#1048): this asserts the BUG. Pulling the trigger with the weapon
    // buried in a creature must hurt it. It does not, because Rapier never
    // re-emits `Started` for a contact that never ended and
    // `TriggeredMeleeWeapon` damages on nothing else. Flip both of these to
    // expect one `Damage` and `beforeHold - 9` when the pull edge also damages
    // what the weapon already overlaps.
    assert.equal(
      await hitPoints(game, monkey.id),
      beforeHold,
      `known-bug baseline (#1048): arming while already in contact deals no damage: ${JSON.stringify(held)}`,
    );
    assert.ok(
      !held.includes("Damage"),
      `known-bug baseline (#1048): no Damage follows an already-touching pull: ${JSON.stringify(held)}`,
    );

    // --- 2. The same weapon and victim, with contact that begins AFTER it. ---
    // A continuous swing, one sample per simulation frame with no dwell - the
    // gesture the existing suites avoid - driving the contact volume down
    // through the torso and out the far side at ~12 m/s.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 2 });
    const windUpAim = await aimAtTorso();
    // Wind up well clear of the victim. This has to clear the *volume*, which
    // is 1.83 units long, so a wind-up that looks generous for a wrench is
    // barely separation at all - retreating only ~1 unit leaves the contact
    // alive and the swing below silently deals nothing.
    const windUp = 3.0;
    const followThrough = -0.9;
    // Every placement re-reads the live victim rather than a torso sampled
    // once up front. The monkey is a live AI that phase 1 just prodded; a
    // gesture staged against a position it has since walked away from swings
    // through empty air, which looks exactly like the bug under test and would
    // make this a false green. Measured drift over the wind-up alone was 1.13
    // units - more than half the contact volume - so this is not hypothetical.
    // Tracking is also the honest gesture: a player swings at where the
    // creature *is*.
    await windUpAim.placeVolumeAt(add(windUpAim.torso, [0, windUp, 0]));
    await game.step({ frames: 20 });

    const swingSequence = await lastSequence(game);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 2 });
    // 21 placements spanning 3.9 units is ~0.2 units per 60 Hz step: about
    // 12 m/s of collider travel, a brisk but ordinary swing. Sub-collider
    // steps are not the point - the volume is never parked on the target, and
    // never dwells.
    const samples = 20;
    for (let i = 0; i <= samples; i++) {
      const t = i / samples;
      const aim = await aimAtTorso();
      await aim.placeVolumeAt(
        add(aim.torso, [0, windUp + (followThrough - windUp) * t, 0]),
      );
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 3 });

    const swung = payloadNames(await messagesTo(game, swingSequence, monkey.id));
    assert.equal(
      swung.filter((payload) => payload === "Damage").length,
      1,
      `one continuous swing should emit exactly one Damage: ${JSON.stringify(swung)}`,
    );
    assert.equal(
      await hitPoints(game, monkey.id),
      beforeHold - 9,
      "the swing should cost the Wrench's authored 9 HP",
    );
  },
);
