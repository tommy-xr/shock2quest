import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { norm } from "./helpers/frontend-menu.js";

// The Cheats page (`shock2vr::ui::cheats_panel`): a second page of the
// Developer screen, reached ONLY from the in-game pause overlay - a cheat acts
// on the running mission, so the main-menu Developer scene fills the same
// framed-button slot with its debug-scene launcher instead.
//
// Negative-first: without the action-rect branch in `pause_menu::target_at`
// the very first assertion here (the page turn off the parameter rows) fails,
// because nothing claims that rect on the pause host.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** SIMR.BIN pause entries: five 179x76 buttons at x=400, top 20, 92px pitch. */
const pauseEntry = (index: number): [number, number] => [400 + 179 / 2, 20 + index * 92 + 76 / 2];
const PAUSE_DEVELOPER = pauseEntry(3);
const PAUSE_CONTINUE = pauseEntry(0);

// GAMELODR.BIN widget rects, shared with the parameter page: the upper framed
// button (rect 2) opens this page, "Done" (rect 3) leaves it, and the rows run
// down the list pane (rect 1: 261,54 202x290) at the debug-scene launcher's
// 19px pitch. The two shipped cheats fit one page, so there is no scroll
// gutter and a row spans the full pane width.
const ACTION: [number, number] = [527 + 96 / 2, 161 + 62 / 2];
const DONE: [number, number] = [527 + 95 / 2, 405 + 62 / 2];
const row = (index: number): [number, number] => [261 + 202 / 2, 54 + index * 19 + 19 / 2];
const RAIN_WEAPONS = row(0);
const RAIN_MODULES = row(1);

/** Click a canvas point with the flat pointer, as a real rising edge. */
async function click(game: GameServer, [x, y]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", norm(x, y));
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
}

async function count(game: GameServer, filter: string): Promise<number> {
  const { entities } = await game.entities.list({ filter, limit: 200 });
  return entities.length;
}

/** Every matching entity's world position, rounded so float noise cannot
 *  make two coincident spawns read as distinct. */
async function positions(game: GameServer, filter: string): Promise<string[]> {
  const { entities } = await game.entities.list({ filter, limit: 200 });
  return entities.map((e) => e.position.map((v) => v.toFixed(2)).join(","));
}

test(
  "the pause overlay's Cheats page rains items into the running mission",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    const wrenchesBefore = await count(game, "Wrench");
    const modulesBefore = await count(game, "EXP");
    const nanitesBefore = await count(game, "Nanites");

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).paused, true);

    // Root -> Developer (the repurposed Options slot) -> Cheats.
    await click(game, PAUSE_DEVELOPER);
    await click(game, ACTION);
    assert.equal((await game.info()).paused, true, "the page turn must not resume");
    assert.equal((await game.info()).mission, "medsci1.mis", "no scene swap");
    await game.screenshot("dev-cheats-page.png");

    // A cheat acts on the paused scene and LEAVES THE OVERLAY UP, so several
    // can be fired before resuming.
    await click(game, RAIN_WEAPONS);
    assert.equal((await game.info()).paused, true, "a cheat must not resume the sim");
    assert.ok(
      (await count(game, "Wrench")) > wrenchesBefore,
      "Rain weapons must add a wrench to the world",
    );

    // Pressing the SAME cheat again must not drop items onto the slots the
    // first press is still occupying - coincident bodies get flung apart by
    // the solver on resume instead of falling. Each rain phases its ring off
    // the last (`RAIN_PHASE_STEP`).
    const afterOne = await positions(game, "Wrench");
    await click(game, RAIN_WEAPONS);
    const afterTwo = await positions(game, "Wrench");
    assert.equal(
      afterTwo.length,
      afterOne.length + 1,
      "a second Rain weapons must add another wrench",
    );
    assert.equal(
      new Set(afterTwo).size,
      afterTwo.length,
      "no two rained wrenches may share a spawn point",
    );

    await click(game, RAIN_MODULES);
    assert.equal(
      (await count(game, "EXP")) - modulesBefore,
      4,
      "Rain modules must add exactly four module stacks",
    );
    assert.equal(
      (await count(game, "Nanites")) - nanitesBefore,
      4,
      "Rain modules must add exactly four nanite piles",
    );

    // Done is the Developer screen's, so it goes back to the parameter rows -
    // not to the root, and not out of the menu.
    await click(game, DONE);
    assert.equal((await game.info()).paused, true, "Done turns the page, not the sim");
    // A second Done leaves the parameters for the root, where Continue resumes.
    await click(game, DONE);
    await click(game, PAUSE_CONTINUE);
    assert.equal((await game.info()).paused, false, "Continue on the root resumes");

    // The rain falls and settles as ordinary world pickups.
    await game.step({ frames: 120 });
    await game.screenshot("dev-cheats-rain.png");
    assert.ok(
      (await count(game, "Wrench")) > wrenchesBefore,
      "the rained items must survive the unpause",
    );
  },
);

test(
  "the main-menu Developer screen keeps the scene launcher in that slot",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 10 });

    // The repurposed Options slot swaps to the Developer scene.
    await click(game, [400 + 179 / 2, 20 + 2 * 76 + 60 / 2]);
    assert.equal((await game.info()).mission, "developer");

    // Same framed button, different page: the launcher, not the cheats. It is
    // still the launcher because there is no mission for a cheat to act on.
    await click(game, ACTION);
    await game.screenshot("dev-cheats-main-menu-launcher.png");
    // "Done" on the launcher returns to the parameters rather than the main
    // menu - which is what tells the two pages apart from out here.
    await click(game, DONE);
    assert.equal((await game.info()).mission, "developer");
    await click(game, DONE);
    assert.equal((await game.info()).mission, "main_menu");
  },
);
