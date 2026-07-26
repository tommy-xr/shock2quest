import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "QuickLoad without a session quicksave leaves the runtime healthy",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8137),
    });

    // Establish the precondition without deleting or overwriting a user's
    // quicksave. A stale save1.sav should fail rather than mask the regression.
    const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
    assert.equal(
      existsSync(join(repoRoot, "save1.sav")),
      false,
      "the test requires a worktree with no pre-existing session quicksave",
    );

    await game.input.trigger("QuickLoad");
    await game.step({ frames: 1 });

    assert.equal((await game.health()).status, "ok");
    assert.equal((await game.info()).mission, "debug_minimal");
    assert.ok(
      game
        .logs()
        .some((line) => line.includes("Unable to load save 'save1.sav'")),
      "missing QuickLoad should be recorded in the runtime log",
    );
  },
);
