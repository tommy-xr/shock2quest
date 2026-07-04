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
    // Chase only re-paths on target drift, so the burst is front-loaded;
    // measure per-frame deltas THROUGH the burst, frame by frame.
    await game.input.trigger("DebugAlertAll");

    let before = (await game.pathfinding.stats())!;
    let total = 0;
    for (let i = 0; i < 12; i++) {
      await game.step({ frames: 1 });
      const after = (await game.pathfinding.stats())!;
      const delta = after.queries - before.queries;
      total += delta;
      assert.ok(
        delta <= QUERIES_PER_FRAME,
        `frame ${i}: ${delta} pathfind queries in one frame (budget is ${QUERIES_PER_FRAME})`,
      );
      before = after;
    }

    // The budget defers work rather than dropping it: across the burst
    // window, multiple AIs must still have been served.
    assert.ok(
      total >= 4,
      `expected several pathfind queries across the burst, got ${total}`,
    );
  },
);
