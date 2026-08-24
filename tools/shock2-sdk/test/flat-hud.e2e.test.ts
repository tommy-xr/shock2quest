import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end smoke test for the flatscreen (non-VR) presentation. The debug
// runtime defaults to flatscreen, so the game builds the screen-space 2D HUD in
// `render_per_eye` instead of the VR forearm panels; this confirms a frame
// renders end-to-end without panicking. Pixel-level correctness of the HUD
// layout is covered by the Rust unit tests in `shock2vr/src/hud/flat_hud.rs`.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "flat presentation: renders a frame with the screen-space HUD",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      // Flatscreen is the debug runtime's default presentation now.
    });

    // Advance enough frames for the mission to load and render.
    await game.step({ frames: 30 });

    const shot = await game.screenshot("flat-hud.png");

    // A real rendered frame at the runtime's fixed resolution. If the flat
    // render path panicked or produced nothing, this would fail.
    assert.deepEqual(shot.resolution, [800, 600]);
    assert.ok(
      shot.size_bytes > 10_000,
      `expected a non-trivial frame, got ${shot.size_bytes} bytes`,
    );
  },
);
