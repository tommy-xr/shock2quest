import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// A second hand takes hold of what the first is holding, anywhere on it, and
// the item is then aimed down the line between the hands.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   SHOCK2_E2E=1 npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The direction a placed item faces, from its body rotation `[x, y, z, w]`.
 * Hand-local -Z is the aim axis the placement solves for. */
function forward(q: [number, number, number, number]): Vec3 {
  const [x, y, z, w] = q;
  const v: Vec3 = [0, 0, -1];
  const t: Vec3 = [
    2 * (y * v[2] - z * v[1]),
    2 * (z * v[0] - x * v[2]),
    2 * (x * v[1] - y * v[0]),
  ];
  return [
    v[0] + w * t[0] + (y * t[2] - z * t[1]),
    v[1] + w * t[1] + (z * t[0] - x * t[2]),
    v[2] + w * t[2] + (x * t[1] - y * t[0]),
  ];
}

function degreesBetween(a: Vec3, b: Vec3): number {
  const dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  return (Math.acos(Math.max(-1, Math.min(1, dot))) * 180) / Math.PI;
}

async function aimAxis(game: GameServer, entityId: number): Promise<Vec3> {
  const body = (await game.physics.bodies({ entityId })).bodies[0];
  assert.ok(body, `entity ${entityId} should have a physics body to read`);
  return forward(body.rotation);
}

/** Close the off-hand at `at` (pawn-local) and let the attach ramp finish. */
async function supportAt(game: GameServer, at: Vec3): Promise<void> {
  await game.input.set("left_hand.position", at);
  await game.input.set("left_hand.squeeze", 0.0);
  await game.step({ frames: 4 });
  await game.input.set("left_hand.squeeze", 1.0);
  await game.step({ frames: 20 });
}

async function releaseSupport(game: GameServer): Promise<void> {
  await game.input.set("left_hand.squeeze", 0.0);
  await game.step({ frames: 20 });
}

test(
  "VR: a second hand supports the melee weapon the first holds, and aims it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 30 });

    const wrench = (await game.entities.list({ filter: "Wrench", limit: 20 }))
      .entities[0];
    assert.ok(wrench, "debug_melee should stock a Wrench");

    // Grab it with the right hand through the production selection ray. The
    // rack stocks several melee weapons, so the hand goes ON the wrench rather
    // than pointing across the rack at whatever is nearest.
    const pawn = (await game.info()).player.position;
    const onWrench: Vec3 = [
      wrench.position[0] - pawn[0],
      wrench.position[1] - pawn[1],
      wrench.position[2] - pawn[2],
    ];
    await game.input.set("right_hand.position", onWrench);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.input.set("right_hand.squeeze", 0.0);
    // Park the off-hand well clear, so nothing is offered before it is moved.
    await game.input.set("left_hand.position", [
      onWrench[0] + 2.0,
      onWrench[1],
      onWrench[2],
    ]);
    await game.input.set("left_hand.squeeze", 0.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 8 });

    const held = (await game.info()).player.right_hand_entity_id;
    assert.equal(held, wrench.id, "the right hand should hold the Wrench");
    assert.equal(
      (await game.info()).player.two_handed,
      false,
      "one hand on the weapon is not two-handed",
    );
    const oneHanded = await aimAxis(game, wrench.id);

    // The melee wield's fitted contact volume runs up out of the tracked hand
    // (`vr_config::melee_contact_offset` parks the body on the weapon head), so
    // the shaft is above the grip.
    await supportAt(game, [onWrench[0], onWrench[1] + 0.6, onWrench[2]]);
    const withSupport = (await game.info()).player;
    assert.equal(withSupport.two_handed, true, "the off-hand should take hold");
    assert.equal(withSupport.two_hand.support_hand, "left");
    assert.equal(
      withSupport.two_hand.support_of,
      wrench.id,
      "the support grip should be on the wrench the other hand holds",
    );
    assert.equal(
      withSupport.two_hand.snapped,
      false,
      "a melee weapon authors no support seat, so the palm latches free",
    );
    assert.equal(
      (await game.info()).player.hand_affordance.left,
      "Grabbable",
      "the supporting hand lights for the grip it holds",
    );

    // A hand that closed in empty space and then swept onto the weapon must not
    // silently attach: taking hold is a rising edge, like every other grip.
    await releaseSupport(game);
    await game.input.set("left_hand.position", [
      onWrench[0] + 2.0,
      onWrench[1],
      onWrench[2],
    ]);
    await game.input.set("left_hand.squeeze", 1.0);
    await game.step({ frames: 6 });
    await game.input.set("left_hand.position", [
      onWrench[0],
      onWrench[1] + 0.6,
      onWrench[2],
    ]);
    await game.step({ frames: 20 });
    assert.equal(
      (await game.info()).player.two_handed,
      false,
      "a fist swept onto the weapon should not grab it without a fresh grip",
    );
    // Opening and closing again on the same spot does take hold.
    await supportAt(game, [onWrench[0], onWrench[1] + 0.6, onWrench[2]]);
    assert.equal(
      (await game.info()).player.two_handed,
      true,
      "a fresh grip on the same spot takes hold",
    );

    // Taking hold where the hands already are changes nothing - the solve is
    // the identity there, which is what makes the attach ease start from zero.
    const attached = await aimAxis(game, wrench.id);
    assert.ok(
      degreesBetween(attached, oneHanded) < 5,
      "attaching with the hands already in place should not move the weapon",
    );

    // MOVING the support hand is what turns it: the weapon swings so the point
    // that hand has hold of follows it round.
    await game.input.set("left_hand.position", [
      onWrench[0],
      onWrench[1],
      onWrench[2] - 0.6,
    ]);
    await game.step({ frames: 20 });
    const moved = await aimAxis(game, wrench.id);
    const turn = degreesBetween(moved, oneHanded);
    assert.ok(
      turn > 45,
      `the weapon should follow the support hand round, turned only ${turn.toFixed(1)} degrees`,
    );

    // Releasing the support hand hands the aim back to the wrist.
    await releaseSupport(game);
    const released = (await game.info()).player;
    assert.equal(released.two_handed, false, "opening the off-hand detaches");
    assert.equal(released.two_hand.support_of, null);
    assert.ok(
      degreesBetween(await aimAxis(game, wrench.id), oneHanded) < 5,
      "the weapon should return to its one-hand aim",
    );
  },
);

test(
  "VR: the off-hand snaps to a shotgun's authored pump, and latches free on a fusion cannon",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    // `sg_h`'s authored support seat, read off the model's own baked arm island
    // (assets/vr_grips.json).
    const PUMP: Vec3 = [-0.382, 0.014, -0.052];

    // Grab `name` off the floor and return its id plus the pawn-local hand
    // position it is now held at - the frame every support probe below is
    // expressed in, since the weapon itself has moved into the hand by then.
    const grab = async (name: string): Promise<[number, Vec3]> => {
      const weapon = await cycleToWeapon(game, (e) => e.name === name, {
        settleFrames: 90,
      });
      const pawnY = (await game.info()).player.position[1];
      const [px, py, pz] = weapon.position;
      const hand: Vec3 = [px + 0.4, py - pawnY, pz];
      await game.input.set("right_hand.position", hand);
      await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
      await game.input.set("right_hand.squeeze", 1.0);
      // Off-hand parked clear of the weapon until the probe moves it.
      await game.input.set("left_hand.position", [hand[0], hand[1] + 3.0, hand[2]]);
      await game.input.set("left_hand.squeeze", 0.0);
      await game.step({ frames: 10 });
      const held = (await game.info()).player.right_hand_entity_id;
      assert.equal(held, weapon.id, `the right hand should hold the ${name}`);
      return [weapon.id, hand];
    };

    // --- the shotgun's pump ---
    const [shotgun, shotgunHand] = await grab("Shotgun");
    const gripAt = (dz: number): Vec3 => [
      shotgunHand[0],
      shotgunHand[1],
      shotgunHand[2] + dz,
    ];

    // Well back down the receiver, away from the pump: a free latch.
    await supportAt(game, gripAt(0.0));
    const free = (await game.info()).player;
    assert.equal(free.two_handed, true, "the receiver is still grabbable");
    assert.equal(
      free.two_hand.snapped,
      false,
      `a palm off the pump latches where it is, got ${JSON.stringify(free.two_hand.support_point)}`,
    );
    await releaseSupport(game);

    // Out on the pump: the authored seat claims it.
    await supportAt(game, gripAt(-0.45));
    const withPump = (await game.info()).player;
    assert.equal(withPump.two_handed, true, "the pump should take a support grip");
    const snapped = withPump.two_hand;
    assert.equal(snapped.snapped, true, "the palm should snap to the authored pump");
    assert.ok(snapped.support_point, "a snapped grip reports where it landed");
    for (const [axis, want] of PUMP.entries()) {
      assert.ok(
        Math.abs(snapped.support_point![axis] - want) < 1e-3,
        `the snap should land on the authored pump ${JSON.stringify(PUMP)}, got ${JSON.stringify(snapped.support_point)}`,
      );
    }
    await releaseSupport(game);
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });

    // --- the fusion cannon: no authored seat anywhere on it ---
    const [fusion, fusionHand] = await grab("Fusion Cannon");
    await supportAt(game, [fusionHand[0], fusionHand[1], fusionHand[2] - 0.4]);
    const held = (await game.info()).player;
    const oversized = held.two_hand;
    assert.equal(
      held.two_handed,
      true,
      "an oversized weapon must still take a second hand anywhere on its body",
    );
    assert.equal(
      oversized.support_of,
      fusion,
      "the support grip should be on the fusion cannon",
    );
    assert.equal(
      oversized.snapped,
      false,
      "the fusion cannon bakes no support hand, so every grip is a free latch",
    );
  },
);
