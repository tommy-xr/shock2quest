import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Position } from "../src/index.js";

// eng2's central-spine tripwire starts the authored Many ride:
//
//   833 -> WhiteOut 1 -> SitDownRightNow / ParalyzePlayers / Cage_Wall
//
// Runtime entity ids change every launch, so every object is resolved through
// its stable mission object id (reported by the SDK as template_id).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRIPWIRE = 833;
const SEAT_1 = 870;
const CAGE_PLATFORM = 976;
const WRENCH = -928;

async function only(game: GameServer, objectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one eng2 object ${objectId}, got ${JSON.stringify(found.map((e) => e.name))}`,
  );
  return found[0];
}

function distance(a: Position | readonly number[], b: Position | readonly number[]): number {
  const av = "x" in a ? [a.x, a.y, a.z] : a;
  const bv = "x" in b ? [b.x, b.y, b.z] : b;
  return Math.hypot(av[0] - bv[0], av[1] - bv[1], av[2] - bv[2]);
}

test(
  "eng2: entering the central tripwire starts the Many ride",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
    });
    await game.step({ frames: 2 });

    const [tripwire, seat, cageBefore] = await Promise.all([
      only(game, TRIPWIRE),
      only(game, SEAT_1),
      only(game, CAGE_PLATFORM),
    ]);

    // Establish an outside -> inside edge through the real Rapier sensor. The
    // debug teleport is marked as locomotion, so TrapNewTripwire receives the
    // same SensorBeginIntersect message as an ordinary corridor crossing.
    const [tx, ty, tz] = tripwire.position;
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz + 4.8 });
    await game.step({ frames: 2 });
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz });

    // WhiteOut 1 uses the authored two-second DelayTime before it seats the
    // player and starts the continuous cage elevator.
    await game.step({ frames: 180 });

    const [playerAfter, cageAfter] = await Promise.all([
      game.player.position(),
      only(game, CAGE_PLATFORM),
    ]);
    assert.ok(
      distance(playerAfter, seat.position) < 6,
      `player should be seated in the ride cage; player=${JSON.stringify(playerAfter)} seat=${JSON.stringify(seat.position)}`,
    );
    assert.ok(
      distance(cageAfter.position, cageBefore.position) > 0.1,
      `ride cage should have left its first waypoint; before=${JSON.stringify(cageBefore.position)} after=${JSON.stringify(cageAfter.position)}`,
    );

    // Let the seated pawn settle onto the moving platform, then hold a full
    // strafe input for one second. ParalyzePlayers must suppress that input;
    // the pawn may move with the cage, but not relative to it.
    await game.step({ frames: 60 });
    const [playerBeforeStrafe, cageBeforeStrafe] = await Promise.all([
      game.player.position(),
      only(game, CAGE_PLATFORM),
    ]);
    await game.input.set("right_hand.thumbstick", [1, 0]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const [playerAfterStrafe, cageAfterStrafe] = await Promise.all([
      game.player.position(),
      only(game, CAGE_PLATFORM),
    ]);
    const relativeBefore = {
      x: playerBeforeStrafe.x - cageBeforeStrafe.position[0],
      y: playerBeforeStrafe.y - cageBeforeStrafe.position[1],
      z: playerBeforeStrafe.z - cageBeforeStrafe.position[2],
    };
    const relativeAfter = {
      x: playerAfterStrafe.x - cageAfterStrafe.position[0],
      y: playerAfterStrafe.y - cageAfterStrafe.position[1],
      z: playerAfterStrafe.z - cageAfterStrafe.position[2],
    };
    assert.ok(
      distance(relativeAfter, relativeBefore) < 1,
      `paralyzed player should not strafe relative to the cage; before=${JSON.stringify(relativeBefore)} after=${JSON.stringify(relativeAfter)}`,
    );

    // The tripwire's second authored branch reaches WhiteOut 2 after 80
    // seconds. Advance from five seconds into the sequence to that boundary,
    // then through WhiteOut's two-second fade-in. A covered frame compresses
    // much smaller than the fully revealed, textured mission frame captured
    // after StandUpAgain has fired and the fade has completed.
    await game.step({ frames: 4_500 });
    await game.step({ frames: 120 });
    const covered = await game.screenshot("eng2-many-ride-whiteout2.png");
    await game.step({ frames: 180 });
    const revealed = await game.screenshot("eng2-many-ride-finished.png");
    assert.deepEqual(covered.resolution, [800, 600]);
    assert.deepEqual(revealed.resolution, [800, 600]);
    assert.ok(
      revealed.size_bytes > covered.size_bytes * 2,
      `WhiteOut 2 should clear after the ride; covered=${covered.size_bytes}B revealed=${revealed.size_bytes}B`,
    );

    // StandUpAgain runs 0.2 seconds after the fully-white relay. Once the view
    // is revealed, the same full strafe input must move the player relative to
    // the cage again.
    const [playerBeforeReleaseStrafe, cageBeforeReleaseStrafe] = await Promise.all([
      game.player.position(),
      only(game, CAGE_PLATFORM),
    ]);
    await game.input.set("right_hand.thumbstick", [1, 0]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const [playerAfterReleaseStrafe, cageAfterReleaseStrafe] = await Promise.all([
      game.player.position(),
      only(game, CAGE_PLATFORM),
    ]);
    const releaseRelativeBefore = {
      x: playerBeforeReleaseStrafe.x - cageBeforeReleaseStrafe.position[0],
      y: playerBeforeReleaseStrafe.y - cageBeforeReleaseStrafe.position[1],
      z: playerBeforeReleaseStrafe.z - cageBeforeReleaseStrafe.position[2],
    };
    const releaseRelativeAfter = {
      x: playerAfterReleaseStrafe.x - cageAfterReleaseStrafe.position[0],
      y: playerAfterReleaseStrafe.y - cageAfterReleaseStrafe.position[1],
      z: playerAfterReleaseStrafe.z - cageAfterReleaseStrafe.position[2],
    };
    assert.ok(
      distance(releaseRelativeAfter, releaseRelativeBefore) > 1,
      `StandUpAgain should restore movement after WhiteOut 2; before=${JSON.stringify(releaseRelativeBefore)} after=${JSON.stringify(releaseRelativeAfter)}`,
    );
  },
);

test(
  "eng2 VR: the Many ride carries an already-held weapon with the player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8159),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 2 });

    // Provision the retail Wrench into the backpack, then equip it through the
    // production carried-weapon action while the actual VR grip stays closed.
    // This establishes the same VirtualHand ownership the Many ride disrupted
    // in the campaign; direct entity messages cannot prove that state.
    const wrench = await game.player.spawnItem(WRENCH);
    await game.input.set("right_hand.squeeze", 1);
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.entity_id,
      "the forced-VR regression must begin with the Wrench physically held",
    );

    const tripwire = await only(game, TRIPWIRE);
    const [tx, ty, tz] = tripwire.position;
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz + 4.8 });
    await game.step({ frames: 2 });
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz });

    // WhiteOut 1 seats and paralyzes the player after two seconds. Suppressing
    // controls must not turn the held squeeze into a synthetic release, and
    // the held entity must follow the long scripted relocation to Seat1.
    await game.step({ frames: 180 });
    const [player, heldWrench] = await Promise.all([
      game.player.position(),
      game.entities.detail(wrench.entity_id),
    ]);
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.entity_id,
      "the Many ride must not release a weapon merely because controls are paralyzed",
    );
    assert.ok(
      distance(player, heldWrench.position) < 3,
      `held Wrench should travel to Seat1 with the player; player=${JSON.stringify(player)} wrench=${JSON.stringify(heldWrench.position)}`,
    );
  },
);

test(
  "eng2: StandUp returns the player from the temporary Many-ride seat",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8160),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 2 });

    const tripwire = await only(game, TRIPWIRE);
    const [tx, ty, tz] = tripwire.position;
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz + 4.8 });
    await game.step({ frames: 2 });
    await game.player.teleport({ x: tx, y: ty + 0.5, z: tz });
    const preSeatPosition = await game.player.position();

    // Advance past the 80-second second-whiteout branch, its two-second
    // fade-in, the 200 ms StandUp delay, and the final fade reveal.
    await game.step({ frames: 5_100 });
    const returned = await game.player.position();
    assert.ok(
      distance(returned, preSeatPosition) < 2,
      `StandUp should restore the pre-seat Engineering pose; before=${JSON.stringify(preSeatPosition)} after=${JSON.stringify(returned)}`,
    );

    // The restored pose is ordinary level geometry and controls are live: a
    // full strafe must move the player instead of leaving them on the isolated
    // continuously-moving cutscene cage.
    await game.input.set("right_hand.thumbstick", [1, 0]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(
      distance(await game.player.position(), returned) > 1,
      "StandUp should restore controls on the normal Engineering route",
    );
  },
);
