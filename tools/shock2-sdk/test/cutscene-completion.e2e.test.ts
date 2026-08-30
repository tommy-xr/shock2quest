import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer } from "../src/index.js";
import { dataRoot } from "./helpers/crf.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// A short authored clip, so the test can step past its whole duration. The
// runtime's own resolver takes the bare classic name through the Anniversary
// layers, so both installs play the same scene.
const SHORT_CUTSCENE = "landing.avi";
const CUTSCENE_STEM = "landing";
const CUTSCENE_LAYERS = ["", "enhanced", "original", "kex"];

/**
 * Whether any install layer ships the clip this test plays, matching the
 * runtime resolver's case-insensitive name comparison (its Anniversary layers
 * are `.ogv` while a classic install has `.avi`).
 */
function installedCutscene(): string | null {
  for (const layer of CUTSCENE_LAYERS) {
    let entries: string[];
    try {
      entries = readdirSync(join(dataRoot(), "cutscenes", layer));
    } catch {
      continue;
    }
    const match = entries.find((entry) =>
      ["avi", "ogv"].some(
        (extension) =>
          entry.toLowerCase() === `${CUTSCENE_STEM}.${extension}`.toLowerCase(),
      ),
    );
    if (match) return join(layer, match);
  }
  return null;
}

test(
  "a cutscene signals completion and hands off to its follow-on scene",
  { skip: !e2eEnabled, timeout: 300_000 },
  async (t) => {
    const installed = installedCutscene();
    if (!installed) {
      // Nothing to assert on an install without the clip. Skip visibly - a
      // silent pass would hide the whole test.
      t.skip(`no ${CUTSCENE_STEM} cutscene installed under ${dataRoot()}`);
      return;
    }

    await using game = await GameServer.launch({ mission: SHORT_CUTSCENE });

    // Mid-playback the cutscene must still own the screen, or "it ended" would
    // be indistinguishable from "it never started".
    await game.step({ frames: 60 });
    assert.match(
      (await game.info()).mission,
      new RegExp(`${CUTSCENE_STEM}\\.(avi|ogv)$`, "i"),
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
