import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Must match PATHFINDING_QUERIES_PER_FRAME in shock2vr::pathfinding.
const QUERIES_PER_FRAME = 2;

test(
  "per-frame budget bounds AI pathfind queries under mass alerting",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    await game.step({ frames: 10 });

    const initial = await game.pathfinding.stats();
    assert.ok(initial, "medsci1 has pathfinding data, stats must be present");

    // Alert every monster at once - the worst-case stampede: dozens of AIs
    // all want their first route within a frame or two of the behavior swap.
    // Queries now run on the pathfinding worker thread, so per-frame stat
    // deltas jitter with the worker's clock; the invariant the budget
    // enforces is the SUBMISSION rate, observable as a cumulative bound on
    // worker searches across the burst window (plus slack for requests
    // already in flight at the window edges).
    const previouslyServed = new Set(
      (await game.pathfinding.aiPaths()).map((path) => path.entity_id),
    );
    await game.input.trigger("DebugAlertAll");

    const FRAMES = 12;
    const before = (await game.pathfinding.stats())!;
    await game.step({ frames: FRAMES });
    // Include one final frame in the budget window; this does not guarantee
    // that the asynchronous worker has drained its submissions.
    await game.step({ frames: 1 });
    const after = (await game.pathfinding.stats())!;
    const total = after.queries - before.queries;

    const SLACK = 4; // in-flight at window edges
    assert.ok(
      total <= QUERIES_PER_FRAME * (FRAMES + 1) + SLACK,
      `${total} pathfind queries across ${FRAMES + 1} frames (budget is ${QUERIES_PER_FRAME}/frame)`,
    );

    // A fixed simulation window cannot promise a minimum amount of worker
    // service. Keep the short-window cap above, then observe bounded progress:
    // four previously unserved AIs must actually receive worker outcomes.
    // Counting distinct records also prevents one AI's retries from satisfying
    // the service assertion. Failed/partial routes still prove a request ran.
    let elapsedFrames = FRAMES + 1;
    let served = (await game.pathfinding.aiPaths()).filter(
      (path) => !previouslyServed.has(path.entity_id),
    );
    let queries = total;
    const MAX_SERVICE_FRAMES = 180;
    for (
      let extra = 0;
      (served.length < 4 || queries < 4) && extra < MAX_SERVICE_FRAMES;
      extra += 6
    ) {
      await game.step({ frames: 6 });
      elapsedFrames += 6;
      queries = (await game.pathfinding.stats())!.queries - before.queries;
      assert.ok(
        queries <= QUERIES_PER_FRAME * elapsedFrames + SLACK,
        `${queries} queries across ${elapsedFrames} frames exceeded the budget`,
      );
      served = (await game.pathfinding.aiPaths()).filter(
        (path) => !previouslyServed.has(path.entity_id),
      );
    }
    assert.ok(
      queries >= 4 && served.length >= 4,
      `expected four newly served AIs within ${elapsedFrames} frames; ` +
        `queries=${queries}, outcomes=${JSON.stringify(served)}`,
    );
  },
);
