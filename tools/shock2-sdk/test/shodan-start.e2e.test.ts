import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for #694. Shodan's authored map-default spawn starts
// above a jagged fall: a 56.31° face initiates a slide, then a 38.66° face
// continues it toward the lower route. The kinematic controller used to erase
// the first face's tangential momentum at that seam and wedge the player at
// (-6.84, 22.66, 110.0), before the finale's opening bridge reveal.
//
// The test starts from the real mission default, waits for the unassisted fall,
// then uses only bounded collision-valid moves to prove the route remains
// playable into the Rickenbacker bridge opening.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "shodan.mis: opening fall reaches the playable bridge route",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8233),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    const spawn = await game.player.position();
    assert.ok(
      Math.abs(spawn.x + 7.8) < 0.1 &&
        Math.abs(spawn.y - 36.4) < 0.1 &&
        Math.abs(spawn.z - 110.0) < 0.1,
      `expected Shodan's authored opening spawn, got ${JSON.stringify(spawn)}`,
    );

    // No player input: gravity must carry the fall through all the authored
    // faces and settle on the lower route.
    await game.step({ frames: 360 });
    const landing = await game.player.position();
    assert.ok(
      landing.x > -3.0 && landing.y < 0.0 && landing.z < 110.0,
      `opening fall should clear the old slope seam and settle on the lower route, got ${JSON.stringify(landing)}`,
    );

    // Walk around the protruding broken geometry into the opening that reveals
    // the intact Rickenbacker bridge. Each hop is bounded and collision-valid.
    for (const targetZ of [landing.z - 4.0, landing.z - 8.0]) {
      const move = await game.player.moveTo({
        x: landing.x,
        y: landing.y,
        z: targetZ,
      });
      assert.ok(
        move.distance_moved > 3.5,
        `lower route should remain walkable, got ${JSON.stringify(move)}`,
      );
      await game.step({ frames: 20 });
    }
    const beforeBridge = await game.player.position();
    const bridgeMove = await game.player.moveTo({
      x: beforeBridge.x + 4.0,
      y: beforeBridge.y,
      z: beforeBridge.z,
    });
    assert.ok(
      bridgeMove.distance_moved > 2.0 && bridgeMove.new_position[0] > 1.0,
      `collision-valid walk should reach the bridge reveal, got ${JSON.stringify(bridgeMove)}`,
    );
  },
);
