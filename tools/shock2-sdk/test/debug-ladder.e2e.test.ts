import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

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

/// Climb the stack station's rungs partway: face them level, push forward
/// for `frames`, then let go of the stick.
async function climbStack(game: GameServer, frames: number) {
  await teleportVerified(game, { x: NEAR_X, y: 1.5, z: STACK_Z });
  await game.step({ frames: 30 });
  const floorY = (await game.player.position()).y;
  const eyeY = floorY + (await game.info()).player.camera_offset[1];
  await game.input.lookAtWorldPoint([-7, eyeY, STACK_Z]);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.thumbstick", [0, 1]);
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
