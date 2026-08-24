import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the quest-bit (objective flag) endpoints. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before GET/POST /v1/quests existed, objective state was
// internal to the script system and unreadable from HTTP - a tester could not
// verify "did objective X complete?". This asserts the read/set round-trip and
// the unknown default that back checkpoint verification.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "quests: read/set round-trip, unknown default, and value validation",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    // A bit the game hasn't set reads as "unknown", and isn't in the list.
    assert.equal(
      await game.quests.get("some.unset.objective"),
      "unknown",
      "an unset quest bit should read as unknown",
    );

    // Set a bit and read it straight back (the primary verification path).
    await game.quests.set("harness.objective", "complete");
    assert.equal(
      await game.quests.get("harness.objective"),
      "complete",
      "a set quest bit should read back its value",
    );

    // Names are case-insensitive (stored lowercased), matching QuestInfo.
    await game.quests.set("MixedCaseObjective", "incomplete");
    assert.equal(await game.quests.get("mixedcaseobjective"), "incomplete");

    const { quests, count } = await game.quests.list();
    assert.equal(count, quests.length);
    const objective = quests.find((q) => q.name === "harness.objective");
    assert.ok(objective, "list should include the set objective");
    assert.equal(objective.value, "complete");
    // The raw bits are exposed losslessly (COMPLETE = 2); scripts compare by raw value.
    assert.equal(objective.bits, 2, "complete objective should have raw bits 2");
    assert.equal(
      quests.find((q) => q.name === "mixedcaseobjective")?.bits,
      1,
      "incomplete objective should have raw bits 1",
    );

    // An invalid value is rejected (not silently stored), and doesn't crash the
    // runtime - a following read still works.
    await assert.rejects(
      // deliberately bad value; cast around the typed API
      game.quests.set("x", "bogus" as never),
      /status 400|invalid quest value/,
      "an invalid quest value should be rejected",
    );
    assert.equal(
      await game.quests.get("harness.objective"),
      "complete",
      "runtime should stay live after a rejected set",
    );

    // Setting a bit back to "unknown" resets it (removed from the list), so it
    // doesn't linger as a touched-but-unknown entry.
    await game.quests.set("harness.objective", "unknown");
    assert.equal(await game.quests.get("harness.objective"), "unknown");
    const after = await game.quests.list();
    assert.ok(
      !after.quests.some((q) => q.name === "harness.objective"),
      "a bit reset to unknown should not appear in the list",
    );
  },
);
