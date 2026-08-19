import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PlayedSound } from "../src/types.js";
import { AIM_AT_PANEL, menuEntry, norm, panelPoint } from "./helpers/frontend-menu.js";

// End-to-end test for the frontend's sound: the original menus are not silent -
// `res/snd/sfx/` ships a looping bed (mloop1), a rollover blip played when the
// cursor moves onto a widget (MROLLOV1) and a select click (MSELECT1).
//
// The audio log is the only headless way to observe audio, so every assertion
// here reads `GET /v1/audio/recent`.
//
// Negative-first: before the frontend played anything, the log stayed empty for
// the whole sequence and every assertion below failed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const HUM = "sfx/mloop1.wav";
const ROLLOVER = "sfx/mrollov1.wav";
const SELECT = "sfx/mselect1.wav";

const NEW_GAME_ENTRY = menuEntry(0);
const LOAD_GAME_ENTRY = menuEntry(1);
// Empty backdrop to the left of the button column - over no widget at all.
const NOTHING = norm(80, 240);

async function played(game: GameServer): Promise<PlayedSound[]> {
  const { sounds } = await game.audio.recent();
  return sounds;
}

/** Samples played after `since` (exclusive), oldest first. */
async function playedSince(game: GameServer, since: number): Promise<string[]> {
  return (await played(game)).filter((s) => s.sequence > since).map((s) => s.sample);
}

async function frontendSoundsSince(game: GameServer, since: number): Promise<string[]> {
  const frontend = new Set([HUM, ROLLOVER, SELECT]);
  return (await playedSince(game, since)).filter((sample) => frontend.has(sample));
}

async function lastSequence(game: GameServer): Promise<number> {
  const sounds = await played(game);
  return sounds.length === 0 ? 0 : sounds[sounds.length - 1].sequence;
}

async function hover(game: GameServer, [x, y]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", [x, y]);
  await game.step({ frames: 5 });
}

test(
  "the frontend plays the original hum, rollover and select sounds",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu", port: 8122 });
    await game.step({ frames: 10 });

    // The bed starts with the screen and loops under it.
    const hum = (await played(game)).find((s) => s.sample === HUM);
    assert.ok(hum, "the menu should start the looping bed");
    assert.equal(hum.still_playing, true, "the bed should still be playing");

    // Moving onto a widget blips once - not once per frame it stays there.
    let since = await lastSequence(game);
    await hover(game, NEW_GAME_ENTRY);
    await game.step({ frames: 30 });
    assert.deepEqual(
      await playedSince(game, since),
      [ROLLOVER],
      "entering an entry should blip exactly once",
    );

    // Off every widget: silence, and re-entry blips again.
    since = await lastSequence(game);
    await hover(game, NOTHING);
    assert.deepEqual(await playedSince(game, since), [], "leaving an entry is silent");

    since = await lastSequence(game);
    await hover(game, LOAD_GAME_ENTRY);
    assert.deepEqual(await playedSince(game, since), [ROLLOVER], "re-entry blips again");

    // Clicking plays the select click, and hands the screen over: the bed ends
    // with the menu rather than bleeding into what replaces it, and the load
    // screen starts its own.
    since = await lastSequence(game);
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 6 });

    assert.deepEqual(
      await playedSince(game, since),
      [SELECT, HUM],
      "the click should play the select sound, and the next screen its own bed",
    );

    const beds = (await played(game)).filter((s) => s.sample === HUM);
    assert.equal(beds.length, 2, "one bed per screen");
    assert.ok(
      beds[0].stopped_at_sim_time !== null,
      "the menu's bed should stop when the menu does",
    );
    assert.equal(beds[1].still_playing, true, "the load screen's bed should be playing");
  },
);

// The VR menu is the same canvas on a world-space panel, driven by a controller
// ray instead of a cursor - so it must make the same sounds at the same moments
// (AGENTS.md: "a canvas must render the same way in flatscreen and in VR").

test(
  "the VR menu plays the same sounds off the controller ray",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: 8123,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    assert.ok(
      (await played(game)).some((s) => s.sample === HUM),
      "the VR menu should start the bed too",
    );

    // Aim at "Load Game" - the ray origin is the button's own world position,
    // so the ray runs straight down -X onto it.
    let since = await lastSequence(game);
    await game.input.set("right_hand.rotation", AIM_AT_PANEL);
    const [, y, z] = panelPoint(LOAD_GAME_ENTRY);
    await game.input.set("right_hand.position", [0, y, z]);
    await game.step({ frames: 10 });
    assert.deepEqual(
      await playedSince(game, since),
      [ROLLOVER],
      "pointing the controller at an entry should blip once",
    );

    // The trigger is VR's click.
    since = await lastSequence(game);
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 8 });
    assert.deepEqual(
      await playedSince(game, since),
      [SELECT, HUM],
      "the trigger should select, and hand the bed over with the screen",
    );
  },
);

test(
  "the bed stops when the menu is replaced without going through a click",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu", port: 8124 });
    await game.step({ frames: 10 });

    const before = (await played(game)).find((s) => s.sample === HUM);
    assert.ok(before, "the menu should start the bed");

    // A level transition driven from outside the screen (the debug runtime's
    // /v1/level, a save load, ...) never reaches the menu's effect handling.
    // The bed loops forever, so nothing but an explicit stop ever ends it -
    // before the scene-exit hook this played on under the mission for good.
    await game.transitionLevel("earth.mis");
    await game.step({ frames: 30 });

    const after = (await played(game)).find((s) => s.sequence === before.sequence);
    assert.ok(
      after && after.stopped_at_sim_time !== null,
      "the bed must stop when the menu is replaced",
    );
    assert.equal(
      (await played(game)).filter((s) => s.sample === HUM).length,
      1,
      "and it must not restart under the mission",
    );
  },
);

test(
  "the pause overlay uses the same hum, rollover and select lifecycle",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal", port: 8125 });
    await game.step({ frames: 10 });

    // Open over empty backdrop so the bed can be observed separately from a
    // rollover. Before the shared shell, PauseMenu owned no FrontendSfx at all.
    await game.input.set("pointer.position", NOTHING);
    let since = await lastSequence(game);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.deepEqual(await frontendSoundsSince(game, since), [HUM]);
    const bed = (await played(game)).find((sound) => sound.sample === HUM);
    assert.ok(bed, "the pause menu should own a looping bed");

    since = await lastSequence(game);
    await hover(game, NEW_GAME_ENTRY); // pause row 0 occupies the same canvas slot
    assert.deepEqual(await frontendSoundsSince(game, since), [ROLLOVER]);

    since = await lastSequence(game);
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 3 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 3 });
    assert.deepEqual(await frontendSoundsSince(game, since), [SELECT]);
    assert.equal((await game.info()).paused, false, "Continue should close the overlay");

    const stoppedBed = (await played(game)).find((sound) => sound.sequence === bed.sequence);
    assert.ok(
      stoppedBed && stoppedBed.stopped_at_sim_time !== null,
      "the pause bed must stop when the overlay closes",
    );
  },
);
