import assert from "node:assert/strict";
import { existsSync, renameSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

/**
 * Where an in-game quicksave lands: `<data_root>/saves/save1.sav` (the same
 * directory every named save uses). The runtime resolves its data root from
 * `DARK_ASSET_PATH`, falling back to `<repo>/Data`.
 */
function quickSavePath(): string {
  const dataRoot =
    process.env.DARK_ASSET_PATH ??
    join(findRepoRoot(process.cwd()) ?? process.cwd(), "Data");
  return join(dataRoot, "saves", "save1.sav");
}

test(
  "QuickLoad without a session quicksave leaves the runtime healthy",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Establish the precondition without destroying a real quicksave: move it
    // aside for the duration and put it back afterwards.
    const quickSave = quickSavePath();
    const parked = `${quickSave}.quickload-missing-test`;
    const hadQuickSave = existsSync(quickSave);
    if (hadQuickSave) renameSync(quickSave, parked);

    try {
      await using game = await GameServer.launch({
        mission: "debug_minimal",
      });

      await game.input.trigger("QuickLoad");
      await game.step({ frames: 1 });

      assert.equal((await game.health()).status, "ok");
      assert.equal((await game.info()).mission, "debug_minimal");
      assert.ok(
        game
          .logs()
          .some(
            (line) =>
              line.includes("Unable to load save") && line.includes("save1.sav"),
          ),
        "missing QuickLoad should be recorded in the runtime log",
      );
    } finally {
      if (hadQuickSave) renameSync(parked, quickSave);
    }
  },
);
