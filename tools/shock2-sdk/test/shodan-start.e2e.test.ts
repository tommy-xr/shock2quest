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

test(
  "shodan.mis: the Citadel-memory portal is open while its window stays solid",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8233) + 1,
    });

    // Resume the reviewed campaign frontier immediately before the portal.
    // Relocation is setup only: both legs through the opening are
    // collision-valid production movement.
    await game.player.teleport({ x: 12.0, y: -2.196, z: -14.0 });
    await game.step({ frames: 10 });
    const start = await game.player.position();
    const east = await game.player.moveTo({
      x: 13.5,
      y: start.y,
      z: start.z,
    });
    assert.ok(
      !east.blocked && east.new_position[0] > 13.3,
      `window collision must not extend into the portal approach, got ${JSON.stringify(east)}`,
    );

    const beforeCrossing = await game.player.position();
    await game.input.lookAtWorldPoint([
      beforeCrossing.x,
      beforeCrossing.y,
      -20.0,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 120 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });

    const across = await game.player.position();
    assert.ok(
      across.z < -18.0 && Math.abs(across.y - start.y) < 0.25,
      `ordinary walking should cross onto the supported floor, got ${JSON.stringify({
        start,
        across,
      })}`,
    );

    // The fix narrows the collision to the authored 3.2-unit OBB; it does not
    // make the unbreakable window itself non-solid.
    const windows = await game.entities.list({
      filter: "UBWin_6x9",
      limit: 100,
    });
    const window = windows.entities.find(
      (entity) => entity.template_id === 1288,
    );
    assert.ok(window, "expected Shodan mission object 1288 (UBWin_6x9)");
    const windowHit = await game.raycast({
      start: [10.0, 0.0, 8.0],
      end: [18.0, 0.0, 8.0],
      collision_groups: ["entity"],
      ignore_sensors: true,
    });
    assert.equal(
      windowHit.entity_id,
      window.id,
      `authored window footprint should stay solid, got ${JSON.stringify(windowHit)}`,
    );
    assert.ok(
      windowHit.hit_point !== null &&
        Math.abs(windowHit.hit_point[0] - 12.8) < 0.02,
      `window west face should use its authored OBB at x=12.8, got ${JSON.stringify(windowHit)}`,
    );
  },
);
