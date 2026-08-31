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
const ANNIVERSARY_LAYERS = ["enhanced", "original", "kex"];

/**
 * Whether the runtime would resolve this test's clip to a real file. Mirrors
 * `resolve_cutscene_path` exactly, including that it is case-insensitive and
 * that its Anniversary layer fallback only ever tries `<stem>.ogv` - accepting
 * a layered `.avi` here would report "installed" for a name the runtime then
 * fails to open.
 */
function installedCutscene(): string | null {
  const found = (layer: string, names: string[]): string | null => {
    let entries: string[];
    try {
      entries = readdirSync(join(dataRoot(), "cutscenes", layer));
    } catch {
      return null;
    }
    const wanted = names.map((name) => name.toLowerCase());
    const match = entries.find((entry) =>
      wanted.includes(entry.toLowerCase()),
    );
    return match ? join(layer, match) : null;
  };

  // The bare `cutscenes/` root takes the requested name, or its `.ogv` twin.
  const rooted = found("", [`${CUTSCENE_STEM}.avi`, `${CUTSCENE_STEM}.ogv`]);
  if (rooted) return rooted;

  for (const layer of ANNIVERSARY_LAYERS) {
    const layered = found(layer, [`${CUTSCENE_STEM}.ogv`]);
    if (layered) return layered;
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
