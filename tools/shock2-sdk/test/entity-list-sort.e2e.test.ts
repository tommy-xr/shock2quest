import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for the entity-list distance sort. Requires game assets
// in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: earth.mis (and rec2.mis) contain an entity at a non-finite
// position, so its `distance` is NaN. list_entities sorted with
// `partial_cmp(..).unwrap_or(Equal)`, which is not a total order - Rust's sort
// panicked ("comparison function does not correctly implement a total order"),
// killing the game thread, so the NEXT request got 503. This asserts listing
// entities in earth succeeds and the runtime stays live.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "entity list does not panic on a NaN distance (earth.mis)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 5 });

    // Before the fix this call crashed the game thread mid-sort.
    const list = await game.entities.list();
    assert.ok(list.total_count >= 0, "entity list should return");

    // The runtime is still live afterward (a crash would 503 here).
    const info = await game.info();
    assert.equal(info.mission, "earth.mis", "runtime should stay live after listing");
  },
);
