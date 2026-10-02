import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { shootTrail, startTrail } from "./helpers/trail.js";

// The debug_ladder scene: one climbing station per lane along z, all at
// x ≈ -7 ahead of the spawn (see shock2vr/src/scenes/debug_ladder.rs). Flat
// climbing (push into the ladder) must ascend every ladder station, a held
// jump must mantle the low block, and the plain wall must block, so the scene
// can stand in for mission geometry in climbing tests.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Approach the near (+X) face from here, or the arch's far (-X) face.
const NEAR_X = -5.5;
const ARCH_FAR_X = -11.5;

type Station = {
  name: string;
  z: number;
  x?: number;
  /// Height of what the climb ends on: a block top the player must stand on,
  /// or (`freestanding`) the ladder's own top, which has nothing to stand on.
  topY: number;
  freestanding?: boolean;
  climbable?: boolean;
  /// No ladder: hold jump and push forward to mantle onto the block.
  mantle?: boolean;
};
const STATIONS: Station[] = [
  { name: "ledge", z: 0, topY: 6.0 },
  { name: "arch", z: 8, topY: 6.4 },
  { name: "arch (far face)", z: 8, x: ARCH_FAR_X, topY: 6.4 },
  { name: "stack", z: -8, topY: 9.0 },
  { name: "short", z: 16, topY: 1.6, freestanding: true },
  { name: "mantle", z: 24, topY: 3.0, mantle: true },
  { name: "wall", z: -16, topY: 6.0, climbable: false },
];

test(
  "debug_ladder: every ladder station climbs, the plain wall does not",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });

    for (const station of STATIONS) {
      const x = station.x ?? NEAR_X;
      await teleportVerified(game, { x, y: 1.5, z: station.z });
      await game.step({ frames: 30 });
      const before = await game.player.position();
      const floorY = before.y;
      // Face the station's ladder level, then push forward into it. Level
      // matters: flat climbing reads a downward pitch as "descend".
      const eyeY = floorY + (await game.info()).player.camera_offset[1];
      const ladderX = station.x === undefined ? -7 : -9;
      await game.input.lookAtWorldPoint([ladderX, eyeY, station.z]);
      await game.step({ frames: 5 });

      // Sample the peak and the height the player holds at the end: a
      // topped-out player keeps walking across its block and eventually
      // drops off the far edge.
      let peak = floorY;
      let standing = false;
      await game.input.set("right_hand.thumbstick", [0, 1]);
      if (station.mantle) await game.input.set("jump", 1);
      for (let i = 0; i < 20; i++) {
        await game.step({ frames: 10 });
        const y = (await game.player.position()).y;
        peak = Math.max(peak, y);
        // Standing on the block: the capsule center sits one floor height
        // above the top, as it did on the floor.
        if (Math.abs(y - (station.topY + floorY)) < 0.1) standing = true;
      }
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.input.set("jump", 0);
      await game.step({ frames: 10 });

      if (station.climbable === false) {
        assert.ok(
          peak - floorY < 0.2,
          `${station.name}: a plain wall must not be climbable (peak y=${peak.toFixed(2)})`,
        );
      } else if (station.freestanding) {
        assert.ok(
          peak > station.topY,
          `${station.name}: pushing into the ladder should climb to its top ` +
            `${station.topY} (peak y=${peak.toFixed(2)})`,
        );
      } else {
        assert.ok(
          standing,
          `${station.name}: should end standing on the block ` +
            `(top ${station.topY}, expected y≈${(station.topY + floorY).toFixed(2)}, ` +
            `peak y=${peak.toFixed(2)})`,
        );
      }
    }
  },
);

/// The stack station's lane: rungs up a 9-tall wall, bare on both sides.
const STACK_Z = -8;

/// Stand at the foot of the ladder in lane `z`, face it level and push
/// forward. Returns the floor height.
async function pushIntoLadder(game: GameServer, z: number) {
  await teleportVerified(game, { x: NEAR_X, y: 1.5, z });
  await game.step({ frames: 30 });
  const floorY = (await game.player.position()).y;
  const eyeY = floorY + (await game.info()).player.camera_offset[1];
  await game.input.lookAtWorldPoint([-7, eyeY, z]);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.thumbstick", [0, 1]);
  return floorY;
}

/// Climb the stack station's rungs partway: face them level, push forward
/// for `frames`, then let go of the stick.
async function climbStack(game: GameServer, frames: number) {
  const floorY = await pushIntoLadder(game, STACK_Z);
  await game.step({ frames });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 2 });
  assert.ok((await game.info()).player.climb.is_climbing, "should be on the ladder");
  return { floorY, start: await game.player.position() };
}

test(
  "debug_ladder: a held ladder holds the player for 120 frames with no input",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });
    const { floorY, start } = await climbStack(game, 60);
    assert.ok(start.y > floorY + 4, `should have climbed (y=${start.y.toFixed(3)})`);
    await game.step({ frames: 120 });
    const end = await game.player.position();
    assert.ok(
      Math.abs(end.y - start.y) < 0.02 && (await game.info()).player.climb.is_climbing,
      `no input should hold on the ladder: y ${start.y.toFixed(3)} -> ${end.y.toFixed(3)}`,
    );
  },
);

test(
  "debug_ladder: strafing along a held ladder over a drop hangs at its side edge",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });
    // Both sides (desktop A/D): the wall is bare beside the rungs, and the
    // floor is further below the feet than a step-off accepts.
    for (const strafe of [1, -1]) {
      const { floorY, start } = await climbStack(game, 60);
      await game.input.set("right_hand.thumbstick", [strafe, 0]);
      await game.step({ frames: 120 });
      const end = await game.player.position();
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.ok(
        Math.abs(end.y - start.y) < 0.02 && (await game.info()).player.climb.is_climbing,
        `strafe ${strafe}: should hang on the ladder, y ${start.y.toFixed(3)} -> ` +
          `${end.y.toFixed(3)} (floor ${floorY.toFixed(3)})`,
      );
      // Slid to the side edge, not off it.
      const w = (end.z - STACK_Z) * strafe;
      assert.ok(w > 0.3 && w < 0.7, `strafe ${strafe}: should stop at the side edge (z=${end.z.toFixed(3)})`);
    }
  },
);

/// Climb rate (world units/s) over `frames`, pushing forward while looking
/// from the eye along `pitchDeg` (positive up) and `headingDeg` off the
/// ladder's face normal. Settles a few frames after re-aiming first.
async function climbRate(game: GameServer, pitchDeg: number, headingDeg: number, frames: number) {
  const { x, y, z } = await game.player.position();
  const eyeY = y + (await game.info()).player.camera_offset[1];
  const [p, h] = [(pitchDeg * Math.PI) / 180, (headingDeg * Math.PI) / 180];
  await game.input.lookAtWorldPoint([
    x - 2 * Math.cos(p) * Math.cos(h),
    eyeY + 2 * Math.sin(p),
    z + 2 * Math.cos(p) * Math.sin(h),
  ]);
  await game.step({ frames: 3 });
  const y0 = (await game.player.position()).y;
  await game.step({ frames });
  const y1 = (await game.player.position()).y;
  assert.ok((await game.info()).player.climb.is_climbing, `pitch ${pitchDeg}: should still be climbing`);
  return ((y1 - y0) * 60) / frames;
}

test(
  "debug_ladder: climb speed is the same looking level, up 45, down 45, and off to the side",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });
    const rates: Record<string, number[]> = {};
    // ledge: one tall ladder collider; stack: eleven stacked rungs.
    for (const [name, z] of [["ledge", 0], ["stack", STACK_Z]] as const) {
      await pushIntoLadder(game, z);
      await game.step({ frames: 10 });
      // Short windows, descent second: the ledge's top-out starts near y 4.5.
      const level = await climbRate(game, 0, 0, 8);
      const down = await climbRate(game, -45, 0, 8);
      const up = await climbRate(game, 45, 0, 8);
      const side = await climbRate(game, 0, 30, 8);
      rates[name] = [level, up, side, down];
      await game.input.set("right_hand.thumbstick", [0, 0]);
      console.log(`${name} climb rates (level, up 45, heading 30, down 45): ${rates[name].map((r) => r.toFixed(3)).join(", ")}`);
    }
    const level = rates.ledge[0];
    for (const [name, [flat, up, side, down]] of Object.entries(rates)) {
      for (const [label, r] of [["level", flat], ["up 45", up], ["heading 30", side], ["down 45", -down]] as const) {
        assert.ok(
          Math.abs(r - level) < 0.05 * level,
          `${name} ${label}: climb rate ${r.toFixed(3)} should match level ${level.toFixed(3)}`,
        );
      }
    }
  },
);

test(
  "debug_ladder: crouch-walking into the mantle block still stands back up",
  // Crouching keeps the standing width, so a crouched body can walk no
  // closer to the block than a standing one fits.
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });
    const mantleZ = STATIONS.find((s) => s.mantle)!.z;
    await teleportVerified(game, { x: NEAR_X, y: 1.5, z: mantleZ });
    await game.step({ frames: 30 });
    await startTrail(game);
    const standing = (await game.player.position()).y;
    const eyeY = standing + (await game.info()).player.camera_offset[1];
    await game.input.lookAtWorldPoint([-7, eyeY, mantleZ]);
    await game.input.set("crouch", 1);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const against = await game.player.position();
    assert.ok(against.x < -6.3, `should have walked up to the block (x=${against.x.toFixed(3)})`);
    await game.input.set("crouch", 0);
    await game.step({ frames: 30 });
    await shootTrail(game, "crouch-walk-stand", { oblique: true });
    const y = (await game.player.position()).y;
    assert.ok(Math.abs(y - standing) < 0.05, `expected to stand back up to ${standing}, got ${y}`);
  },
);
