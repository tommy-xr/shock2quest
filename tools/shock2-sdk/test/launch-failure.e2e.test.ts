import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Verifies that a game-thread crash during launch is surfaced as a useful
// error (rather than hanging or silently "succeeding" against the HTTP
// server, which starts before - and can outlive - the game thread).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "launching a crashing mission rejects with the runtime's output",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    // A nonexistent mission file panics the game thread during init.
    await assert.rejects(
      GameServer.launch({
        mission: "doesnotexist.mis",
      }),
      (error: Error) => {
        assert.match(error.message, /debug_runtime failed to start/);
        assert.match(
          error.message,
          /process exited early \(code/,
          "error should report the process exit",
        );
        assert.match(
          error.message,
          /panicked at/,
          "error should include the panic message from the runtime logs",
        );
        assert.match(
          error.message,
          /stack backtrace:/,
          "error should include the callstack (RUST_BACKTRACE=1 is set by launch)",
        );
        return true;
      },
    );
  },
);
