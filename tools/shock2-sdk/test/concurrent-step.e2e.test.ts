import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, HttpError } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "concurrent step is rejected without interrupting the in-flight step",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8115),
    });

    // Keep the rejection handled from the moment the request is launched:
    // the buggy runtime drops this reply as soon as the second request arrives.
    const inFlight = game.step({ frames: 10_000 }).then(
      (result) => ({ result, error: undefined }),
      (error: unknown) => ({ result: undefined, error }),
    );

    await game.waitFor(
      () =>
        game
          .logs()
          .some((line) => line.includes("Starting step: 10000 frames")),
      {
        timeoutMs: 10_000,
        intervalMs: 1,
        description: "the 10,000-frame step to start",
      },
    );

    await assert.rejects(
      game.step({ frames: 2 }),
      (error: unknown) =>
        error instanceof HttpError &&
        error.status === 409 &&
        /step already in progress/i.test(error.body),
      "the overlapping step should receive a clear conflict response",
    );

    const first = await inFlight;
    assert.ifError(first.error);
    assert.equal(first.result?.frames_advanced, 10_000);

    const later = await game.step({ frames: 2 });
    assert.equal(later.frames_advanced, 2);
    assert.equal(
      later.new_frame_index,
      first.result!.new_frame_index + 2,
      "a later step should work after the original request completes",
    );
  },
);
