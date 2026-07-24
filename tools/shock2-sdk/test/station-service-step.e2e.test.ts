import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The service hall's lower floor and upper tread are joined in one level
// trimesh. A tiny vertical component in the riser's face normal makes Rapier
// report that lateral face as a zero-time hit during the upward step probe.
//
// Negative-first: before the headroom contact classification fix, ordinary
// thumbstick locomotion stalls at z~6.44 with the player center still at
// y~-3.80. Crossing the 0.6u rise puts the center near y=-3.20 and allows
// forward progress beyond z=6.8.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "station.mis: ordinary locomotion steps onto the service-hall tread",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8349),
    });
    await game.step({ frames: 5 });

    // Teleport only to stage before the blocker. All progress through and
    // beyond it uses the same continuous thumbstick path as desktop gameplay.
    await game.player.teleport({ x: 14.04, y: -3.8, z: 4.7 });
    await game.step({ frames: 30 });

    await game.input.set("right_hand.thumbstick", [0.787, 0.616]);
    await game.step({ frames: 12 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const approach = await game.player.position();
    assert.ok(
      approach.x < 13.6 && approach.z > 6.2,
      `expected to reach the service step approach, got ` +
        `(${approach.x.toFixed(3)}, ${approach.y.toFixed(3)}, ${approach.z.toFixed(3)})`,
    );

    await game.input.set("right_hand.thumbstick", [0.957, 0.29]);
    await game.step({ frames: 12 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const crossed = await game.player.position();
    assert.ok(
      crossed.y > -3.4,
      `expected the player center on the upper tread (y > -3.4), got ` +
        `(${crossed.x.toFixed(3)}, ${crossed.y.toFixed(3)}, ${crossed.z.toFixed(3)})`,
    );
    assert.ok(
      crossed.z > 6.8,
      `expected forward progress past the riser (z > 6.8), got ` +
        `(${crossed.x.toFixed(3)}, ${crossed.y.toFixed(3)}, ${crossed.z.toFixed(3)})`,
    );
  },
);
