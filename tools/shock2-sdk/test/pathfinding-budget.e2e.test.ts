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
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8104),
    });

    await game.step({ frames: 10 });

    const initial = await game.pathfinding.stats();
    assert.ok(initial, "medsci1 has pathfinding data, stats must be present");

    // Alert every monster at once - the worst-case stampede: dozens of AIs
    // all want their first route within a frame or two of the behavior swap.
    // Queries now run on the pathfinding worker thread, so per-frame stat
    // deltas jitter with the worker's clock; the invariant the budget
    // enforces is the SUBMISSION rate, observable as a cumulative bound on
    // completed queries across the burst window (plus slack for requests
    // already in flight at the window edges).
    await game.input.trigger("DebugAlertAll");

    const FRAMES = 12;
    const before = (await game.pathfinding.stats())!;
    await game.step({ frames: FRAMES });
    // Give the worker a beat to drain the final frame's submissions, then
    // step once more so its results are observable.
    await game.step({ frames: 1 });
    const after = (await game.pathfinding.stats())!;
    const total = after.queries - before.queries;

    const SLACK = 4; // in-flight at window edges
    assert.ok(
      total <= QUERIES_PER_FRAME * (FRAMES + 1) + SLACK,
      `${total} pathfind queries across ${FRAMES + 1} frames (budget is ${QUERIES_PER_FRAME}/frame)`,
    );

    // The budget defers work rather than dropping it: across the burst
    // window, multiple AIs must still have been served.
    assert.ok(
      total >= 4,
      `expected several pathfind queries across the burst, got ${total}`,
    );
  },
);
