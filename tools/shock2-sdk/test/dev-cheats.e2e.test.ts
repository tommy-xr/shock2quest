import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  DEV_ACTION,
  DEV_DONE,
  clickCanvas as click,
  pauseEntry,
} from "./helpers/frontend-menu.js";

// The Cheats page (`shock2vr::ui::cheats_panel`): a second page of the
// Developer screen, reached ONLY from the in-game pause overlay - a cheat acts
// on the running mission, so the main-menu Developer scene fills the same
// framed-button slot with its debug-scene launcher instead.
//
// Negative-first: without the action-rect branch in `pause_menu::target_at`
// the very first assertion here (the page turn off the parameter rows) fails,
// because nothing claims that rect on the pause host.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const PAUSE_DEVELOPER = pauseEntry(3);
const PAUSE_CONTINUE = pauseEntry(0);

// The rows run down the developer frame's list pane (GAMELODR.BIN rect 1:
// 261,54 202x290) at the shared name-list pitch. The pane holds 14 rows, so
// the shipped cheats fit one page: no scroll gutter, and a row spans the full
// pane width.
const row = (index: number): [number, number] => [261 + 202 / 2, 54 + index * 19 + 19 / 2];
const RAIN_WEAPONS = row(0);
const RAIN_MODULES = row(1);
const HUNT_ME = row(2);
const CALM_ALL = row(4);

/**
 * Open the Cheats page from a running mission, click one row, close back out
 * and let the sim run - then read whatever the caller is watching. The AI rows
 * land through the script world, so their effect is only observable once the
 * scene updates again.
 */
async function clickCheatAndResume<T>(
  game: GameServer,
  cheat: [number, number],
  read: () => Promise<T>,
): Promise<T> {
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 10 });
  await click(game, PAUSE_DEVELOPER);
  await click(game, DEV_ACTION);
  await click(game, cheat);
  await click(game, DEV_DONE);
  await click(game, DEV_DONE);
  await click(game, PAUSE_CONTINUE);
  await game.step({ frames: 60 });
  return read();
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
    await click(game, DEV_ACTION);
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
    await click(game, DEV_DONE);
    assert.equal((await game.info()).paused, true, "Done turns the page, not the sim");
    // A second Done leaves the parameters for the root, where Continue resumes.
    await click(game, DEV_DONE);
    await click(game, PAUSE_CONTINUE);
    assert.equal((await game.info()).paused, false, "Continue on the root resumes");

    // The rain falls and settles as ordinary world pickups.
    await game.step({ frames: 120 });
    await game.screenshot("dev-cheats-rain.png");
    assert.ok(
      (await count(game, "Wrench")) > wrenchesBefore,
      "the rained items must survive the unpause",
    );

    // The AI rows reach the same `SetAllAIAlertness` the desktop's Alt+G /
    // Alt+C reach - which on a headset, with no keyboard, is the only way to
    // reach them at all. The effect dispatches a message the AI scripts read
    // on their next update, so the level only moves once the scene is running
    // again: each row is clicked, the overlay closed, and the sim stepped.
    const hybrids = (await game.entities.list({ filter: "OG-", limit: 20 })).entities.filter(
      (e) => e.name.startsWith("OG-"),
    );
    assert.ok(hybrids.length >= 1, `expected hybrids in medsci1, got ${hybrids.length}`);
    const alertness = async (): Promise<string | undefined> => {
      const detail = await game.entities.detail(hybrids[0].id);
      return detail.properties.find((p) => p.name === "AIAlertness")?.value;
    };

    const calmed = await clickCheatAndResume(game, CALM_ALL, alertness);
    const hunting = await clickCheatAndResume(game, HUNT_ME, alertness);
    assert.notEqual(hunting, calmed, `"Hunt me" must raise alertness from ${calmed}`);
    assert.equal(
      await clickCheatAndResume(game, CALM_ALL, alertness),
      calmed,
      '"Calm all" must put it back',
    );
  },
);
