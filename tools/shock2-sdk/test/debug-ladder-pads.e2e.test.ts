import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// debug_ladder's teleport pads: the repro stations are enclosed, so each has a
// magenta go pad on the open floor beside its sign and exit pads inside. The
// capped test walks (thumbstick, no HTTP teleport) from the spawn onto its go
// pad and its exit pad; the jump-grab tests cover a crouched arrival. Positions mirror
// shock2vr/src/scenes/debug_ladder.rs (lane frame: d = x - STATION_FACE_X,
// w = z - lane).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

type Pos = { x: number; y: number; z: number };

const STATION_FACE_X = -7.0;
const STANDING_LIFT = 1.244;
const CAPPED_LANE = 36;
/** GO_PAD: d 10, w 3, on the floor. */
const GO_PAD: Pos = { x: STATION_FACE_X + 10, y: 0, z: CAPPED_LANE + 3 };
/** The capped station's start pad (d 0.729), standing. */
const START: Pos = {
  x: STATION_FACE_X + 0.729,
  y: STANDING_LIFT,
  z: CAPPED_LANE,
};
/** Its exit pad in the pit (d 1.7, w 0.55). */
const EXIT_PAD: Pos = { x: STATION_FACE_X + 1.7, y: 0, z: CAPPED_LANE + 0.55 };
/** Where an exit lands: 2 past the go pad, standing. */
const BACK: Pos = { x: GO_PAD.x + 2, y: STANDING_LIFT, z: GO_PAD.z };

/** Jump grab: its pipe has 1.6 headroom, so the go pad takes a crouched body. */
const JUMP_LANE = 96;
const PIPE_TOP = 14.8;
const CROUCHED_LIFT = 0.594;
const CEILING = 16.4;
/** The shaft floor's exit pad (d 4.5, w -0.7), and where exits land. */
const JUMP_FLOOR_EXIT: Pos = {
  x: STATION_FACE_X + 4.5,
  y: 0,
  z: JUMP_LANE - 0.7,
};
const JUMP_BACK: Pos = { ...BACK, z: JUMP_LANE + 3 };
const JUMP_GO_PAD: Pos = { ...GO_PAD, z: JUMP_LANE + 3 };
const JUMP_START: Pos = {
  x: STATION_FACE_X + 1.286,
  y: PIPE_TOP + CROUCHED_LIFT,
  z: JUMP_LANE - 1.789,
};

const flat = (p: Pos, q: Pos) => Math.hypot(p.x - q.x, p.z - q.z);

/// Walk toward `target` with the stick, re-aiming the head every frame,
/// until the body is moved to within 0.3 of `arrival` (a pad fired). Returns
/// the first position seen there, or fails after `maxFrames`.
async function walkUntilArrival(
  game: GameServer,
  target: Pos,
  arrival: Pos,
  maxFrames = 900,
): Promise<Pos> {
  let p = await game.player.position();
  for (let frame = 0; frame < maxFrames; frame++) {
    // `head.look` yaw 0 gazes along -X; the spawn pawn is unrotated. Level
    // gaze, so pitch does not change the walk.
    const yaw =
      (Math.atan2(-(target.z - p.z), -(target.x - p.x)) * 180) / Math.PI;
    await game.input.set("head.look", [yaw, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 1 });
    p = await game.player.position();
    if (flat(p, arrival) < 0.3) {
      await game.input.set("right_hand.thumbstick", [0, 0]);
      return p;
    }
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.fail(
    `never arrived at (${arrival.x}, ${arrival.z}) walking to (${target.x}, ${target.z}); ` +
      `ended at (${p.x.toFixed(2)}, ${p.y.toFixed(2)}, ${p.z.toFixed(2)})`,
  );
}

/// Stand still on the arrival spot: it must not fire another pad.
async function assertStaysAt(game: GameServer, spot: Pos, label: string) {
  await game.step({ frames: 90 });
  const p = await game.player.position();
  assert.ok(
    flat(p, spot) < 0.3 && Math.abs(p.y - spot.y) < 0.2,
    `${label}: expected to rest at (${spot.x}, ${spot.y}, ${spot.z}), ` +
      `ended at (${p.x.toFixed(2)}, ${p.y.toFixed(2)}, ${p.z.toFixed(2)})`,
  );
}

for (const vr of [false, true]) {
  test(
    `debug_ladder pads (${vr ? "VR" : "flat"}): walking onto the capped go pad enters the station, its exit pad leaves it`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_ladder",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 5 });

      await walkUntilArrival(game, GO_PAD, START);
      await assertStaysAt(game, START, "on the start pad");

      await walkUntilArrival(game, EXIT_PAD, BACK, 300);
      await assertStaysAt(game, BACK, "back beside the go pad");
    },
  );
}

// The jump-grab pipe has 1.6 headroom under the 16.4 ceiling: its go pad
// takes a standing player and lands them crouched, as a crouched save loads.
// Walking off the pipe to the floor below stands them up unless crouch is held.
for (const { vr, held } of [
  { vr: false, held: false },
  { vr: false, held: true },
  { vr: true, held: false },
]) {
  test(
    `debug_ladder pads (${vr ? "VR" : "flat"}): the jump-grab go pad lands a standing player crouched on the pipe; ` +
      `off the pipe they ${held ? "stay crouched while crouch is held" : "stand back up"}`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_ladder",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 5 });
      // The walk from the spawn is the capped test's; start 3 short of the pad.
      await game.player.teleport({
        x: JUMP_GO_PAD.x + 3,
        y: STANDING_LIFT,
        z: JUMP_GO_PAD.z,
      });
      await game.step({ frames: 10 });

      // Standing, crouch not held: arrives in the crouched capsule, crown
      // (centre + crouched half height 0.56) under the ceiling.
      const arrived = await walkUntilArrival(
        game,
        JUMP_GO_PAD,
        JUMP_START,
        120,
      );
      assert.ok(
        Math.abs(arrived.y - JUMP_START.y) < 0.1 && arrived.y + 0.56 < CEILING,
        `arrived: ${JSON.stringify(arrived)}`,
      );
      await assertStaysAt(
        game,
        JUMP_START,
        "crouched on the pipe, holding still",
      );

      // Walk off the pipe (+X) onto the shaft floor, where there is headroom.
      if (held) await game.input.set("crouch", 1);
      await game.input.set("head.look", [180, 0]);
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 30 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.step({ frames: 120 });
      const floor = await game.player.position();
      const expected = held ? CROUCHED_LIFT : STANDING_LIFT;
      assert.ok(
        Math.abs(floor.y - expected) < 0.05,
        `on the shaft floor: expected body centre ${expected}, got ${JSON.stringify(floor)}`,
      );

      // The shaft floor's exit pad leads back out, standing.
      if (!held) {
        await walkUntilArrival(game, JUMP_FLOOR_EXIT, JUMP_BACK, 300);
        await assertStaysAt(game, JUMP_BACK, "back beside the go pad");
      }
    },
  );
}
