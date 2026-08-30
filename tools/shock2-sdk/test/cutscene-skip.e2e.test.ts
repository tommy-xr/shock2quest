import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Holding either trigger skips the movie. Negative-first: before the skip, both
// tests below sat on "cs1.avi" for the whole 400-frame budget (the intro runs
// for minutes), so the "playback ended early" assertions failed.
//
// Booting the runtime straight into a cutscene gives it `ShowMainMenu` as its
// follow-on, so the menu appearing is the observable skip.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The intro movie, ~2:40 long - far longer than anything stepped here. */
const INTRO = "cs1.avi";
/** Matches `SKIP_HOLD_DURATION` (1.5s) with room for the frame it lands on. */
const HOLD_FRAMES = 120;

test(
  "holding a trigger skips a cutscene to its follow-on",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: INTRO });
    await game.step({ frames: 10 });
    assert.equal((await game.info()).mission, INTRO, "the movie should be playing");

    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: HOLD_FRAMES });

    assert.equal(
      (await game.info()).mission,
      "main_menu",
      "a held trigger should end the movie early and hand off to its follow-on",
    );
  },
);

test(
  "a released trigger restarts the hold, and a movie left alone keeps playing",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: INTRO });
    await game.step({ frames: 10 });

    // Two 1s holds: enough to skip if the hold survived the release, since the
    // threshold is 1.5s.
    await game.input.set("left_hand.trigger", 1.0);
    await game.step({ frames: 60 });
    await game.input.set("left_hand.trigger", 0.0);
    await game.step({ frames: 10 });
    await game.input.set("left_hand.trigger", 1.0);
    await game.step({ frames: 60 });

    assert.equal(
      (await game.info()).mission,
      INTRO,
      "two part-holds should not add up to a skip",
    );

    // ... and untouched, the movie plays on rather than being dismissed by the
    // resting trigger value.
    await game.input.set("left_hand.trigger", 0.0);
    await game.step({ frames: 300 });
    assert.equal(
      (await game.info()).mission,
      INTRO,
      "an untouched movie should still be playing",
    );
  },
);
