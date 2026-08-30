import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// A short authored clip, so the test can step past its whole duration. The
// runtime's own resolver takes the bare classic name through the Anniversary
// layers, so both installs play the same scene.
const SHORT_CUTSCENE = "landing.avi";
const CUTSCENE_LAYERS = ["", "enhanced", "original", "kex"];

function dataRoot(): string {
  return (
    process.env.DARK_ASSET_PATH ??
    join(findRepoRoot(process.cwd()) ?? process.cwd(), "Data")
  );
}

/** Whether any install layer actually ships the clip this test plays. */
function cutsceneIsInstalled(): boolean {
  return CUTSCENE_LAYERS.some((layer) =>
    ["avi", "ogv"].some((extension) =>
      existsSync(
        join(
          dataRoot(),
          "cutscenes",
          layer,
          `${SHORT_CUTSCENE.replace(/\.avi$/, "")}.${extension}`,
        ),
      ),
    ),
  );
}

test(
  "a cutscene signals completion and hands off to its follow-on scene",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    if (!cutsceneIsInstalled()) {
      // Nothing to assert on an install without the clip; skipping beats a
      // failure that says nothing about the change.
      return;
    }

    await using game = await GameServer.launch({ mission: SHORT_CUTSCENE });

    // Mid-playback the cutscene must still own the screen, or "it ended" would
    // be indistinguishable from "it never started".
    await game.step({ frames: 60 });
    const playing = (await game.info()).mission;
    assert.match(
      playing,
      /landing\.(avi|ogv)$/i,
      "the cutscene should be the active scene while it plays",
    );

    // landing is under 5s; 12s of fixed-timestep stepping clears it with room
    // for a slower re-encode.
    await game.step({ frames: 12 * 60 });

    assert.equal(
      (await game.info()).mission,
      "main_menu",
      "a finished cutscene should hand off to the main menu",
    );
    assert.equal((await game.health()).status, "ok");
  },
);
