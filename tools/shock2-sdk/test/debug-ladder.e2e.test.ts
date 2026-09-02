import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// The debug_ladder scene: one climbing station per lane along z, all at
// x ≈ -7 ahead of the spawn (see shock2vr/src/scenes/debug_ladder.rs). Flat
// climbing (push into the ladder) must ascend every ladder station and be
// blocked by the plain wall, so the scene can stand in for mission geometry
// in climbing tests.
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
};
const STATIONS: Station[] = [
  { name: "ledge", z: 0, topY: 6.0 },
  { name: "arch", z: 8, topY: 6.4 },
  { name: "arch (far face)", z: 8, x: ARCH_FAR_X, topY: 6.4 },
  { name: "stack", z: -8, topY: 9.0 },
  { name: "short", z: 16, topY: 1.6, freestanding: true },
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
      for (let i = 0; i < 20; i++) {
        await game.step({ frames: 10 });
        const y = (await game.player.position()).y;
        peak = Math.max(peak, y);
        // Standing on the block: the capsule center sits one floor height
        // above the top, as it did on the floor.
        if (Math.abs(y - (station.topY + floorY)) < 0.1) standing = true;
      }
      await game.input.set("right_hand.thumbstick", [0, 0]);
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
          `${station.name}: pushing into the ladder should top out onto the block ` +
            `(top ${station.topY}, expected y≈${(station.topY + floorY).toFixed(2)}, ` +
            `peak y=${peak.toFixed(2)})`,
        );
      }
    }
  },
);
