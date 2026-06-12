import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "pathfinding test scenario: inject actions and verify computed path",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8091),
    });

    // Give the mission a couple of frames to settle.
    await game.step({ frames: 2 });

    // The action vocabulary should include the pathfinding cycle.
    const actions = await game.input.actions();
    assert.ok(
      actions.includes("PathfindingTestCycle"),
      `expected PathfindingTestCycle in ${actions}`,
    );

    // Initial state: waiting for a start position.
    const initial = await game.pathfindingTest.status();
    assert.equal(initial.state, "WaitingForStart");
    assert.equal(initial.test_path_waypoints, 0);

    // First cycle sets the start at the player's position.
    await game.input.trigger("PathfindingTestCycle");
    await game.waitFor(
      async () => (await game.pathfindingTest.status()).state === "WaitingForGoal",
      { description: "pathfinding state to reach WaitingForGoal" },
    );

    // Move the player so start != goal, then cycle again to compute a path.
    const start = await game.player.position();
    await game.player.teleport({ x: start.x + 5, y: start.y, z: start.z + 5 });
    await game.pathfindingTest.cycle();

    const final = await game.waitFor(
      async () => {
        const status = await game.pathfindingTest.status();
        return status.state === "ShowingPath" ? status : undefined;
      },
      { description: "pathfinding state to reach ShowingPath" },
    );
    assert.ok(
      final.test_path_waypoints > 0,
      `expected a computed path with waypoints, got ${final.test_path_waypoints}`,
    );

    // Unknown actions are rejected with a helpful message.
    await assert.rejects(
      game.input.trigger("NotARealAction"),
      /Unknown action 'NotARealAction'/,
    );
  },
);
