import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The debug_ladder scene: one climbing station per lane along z, all at
// x ≈ -7 ahead of the spawn (see shock2vr/src/scenes/debug_ladder.rs). Flat
// climbing (push into the ladder) must ascend every ladder station and be
// blocked by the plain wall, so the scene can stand in for mission geometry
// in climbing tests.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Standing capsule center above the floor.
const FLOOR_Y = 1.24;

type Station = { name: string; z: number; blockTop: number | null };
const STATIONS: Station[] = [
  { name: "ledge", z: 0, blockTop: 6.0 },
  { name: "arch", z: 8, blockTop: 6.4 },
  { name: "stack", z: -8, blockTop: 9.0 },
  { name: "short", z: 16, blockTop: 1.6 },
  { name: "wall", z: -16, blockTop: null },
];

test(
  "debug_ladder: every ladder station climbs, the plain wall does not",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });

    for (const station of STATIONS) {
      await game.player.teleport({ x: -5.5, y: 1.5, z: station.z });
      await game.step({ frames: 30 });
      const before = await game.player.position();
      assert.ok(
        Math.abs(before.y - FLOOR_Y) < 0.1,
        `${station.name}: expected to start on the floor, got y=${before.y}`,
      );

      // Face -X (the spawn heading) and push forward into the ladder. Sample
      // the peak: a topped-out player keeps walking and drops off the far
      // edge of its block.
      let peak = before.y;
      await game.input.set("right_hand.thumbstick", [0, 1]);
      for (let i = 0; i < 12; i++) {
        await game.step({ frames: 10 });
        peak = Math.max(peak, (await game.player.position()).y);
      }
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.step({ frames: 10 });

      if (station.blockTop === null) {
        assert.ok(
          peak - before.y < 0.2,
          `${station.name}: a plain wall must not be climbable (peak y=${peak.toFixed(2)})`,
        );
      } else {
        assert.ok(
          peak > station.blockTop,
          `${station.name}: pushing into the ladder should reach above its block top ` +
            `${station.blockTop} (peak y=${peak.toFixed(2)})`,
        );
      }
    }
  },
);
