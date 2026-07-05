import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the teleport confirmation position. Requires game assets
// in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: teleport_player set the physics body but not PlayerInfo.pos,
// which only re-synced on the next update(). So the teleport endpoint's own
// confirmation read reported the STALE pre-teleport position (~the spawn point)
// even though the teleport itself worked. This asserts the reported position
// matches the request, with no intervening step.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "teleport reports the actual new position immediately",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8103),
    });
    await game.step({ frames: 2 });

    const target = { x: 5, y: 1, z: 5 };
    const result = await game.player.teleport(target);
    assert.equal(result.success, true);

    // The confirmation position must reflect the teleport, not the stale spawn.
    const [x, y, z] = result.new_position;
    assert.ok(
      Math.abs(x - target.x) < 0.01 &&
        Math.abs(y - target.y) < 0.01 &&
        Math.abs(z - target.z) < 0.01,
      `teleport should report the new position ~${JSON.stringify(target)}, got [${x}, ${y}, ${z}]`,
    );

    // And a follow-up query agrees.
    const pos = await game.player.position();
    assert.ok(
      Math.abs(pos.x - target.x) < 0.01 &&
        Math.abs(pos.y - target.y) < 0.01 &&
        Math.abs(pos.z - target.z) < 0.01,
      `player position should be ~${JSON.stringify(target)}, got ${JSON.stringify(pos)}`,
    );
  },
);
