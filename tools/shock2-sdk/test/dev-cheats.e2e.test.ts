import { launchDeveloperGame } from "./helpers/developer-game.js";
import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  DEV_ACTION,
  PAUSE_DEVELOPER,
  DEV_DONE,
  clickCanvas as click,
  pauseEntry,
  vrClickCanvasPoint,
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

const PAUSE_CONTINUE = pauseEntry(0);

// The rows run down the developer frame's list pane (GAMELODR.BIN rect 1:
// 261,54 202x290) at the shared name-list pitch. The pane holds 14 rows, so
// the shipped cheats fit one page: no scroll gutter, and a row spans the full
// pane width.
const row = (index: number): [number, number] => [261 + 202 / 2, 54 + index * 19 + 19 / 2];
const RAIN_WEAPONS = row(1);
const RAIN_MODULES = row(2);
const HUNT_ME = row(3);
const CALM_ALL = row(5);
const MAX_STATS = row(6);
const RAIN_GADGETS = row(10);

for (const vr of [false, true]) {
  test(`Rain gadgets spawns usable devices and implants (${vr ? "VR" : "flat"})`, {
    skip: !e2eEnabled,
    timeout: 120_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [] });
    const clickPoint = vr ? vrClickCanvasPoint : click;
    const gadgets = [
      "French-Epstein Device", "Molec. Analyzer", "ICE Pick",
      "BrawnBoost", "EndurBoost", "SwiftBoost", "SmartBoost", "LabAssistant",
      "ExperTech", "WormBlood", "WormHeart", "WormMind",
    ];
    await game.step({ frames: 1 });
    const before = new Set((await game.entities.list({ limit: 200 })).entities.map(e => e.id));
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    await clickPoint(game, PAUSE_DEVELOPER);
    await clickPoint(game, DEV_ACTION);

    for (const clicks of [1, 2]) {
      await clickPoint(game, RAIN_GADGETS);
      assert.equal((await game.info()).paused, true, "spawning leaves the menu open");
      const spawned = (await game.entities.list({ limit: 200 })).entities.filter(e => !before.has(e.id));
      assert.deepEqual(
        spawned.map(e => e.name).sort(),
        Array.from({ length: clicks }, () => gadgets).flat().sort(),
        "each click adds exactly one of each device and supported implant",
      );
      assert.equal(new Set(spawned.map(e => e.position.join(","))).size, spawned.length,
        "repeated clicks must use distinct spawn points");
    }

    await clickPoint(game, DEV_DONE);
    await clickPoint(game, DEV_DONE);
    await clickPoint(game, PAUSE_CONTINUE);
    await game.step({ frames: 120 });
    assert.equal((await game.info()).paused, false);
    const settled = (await game.entities.list({ limit: 200 })).entities.filter(e => !before.has(e.id));
    assert.deepEqual(settled.map(e => e.name).sort(), [...gadgets, ...gadgets].sort(),
      "gadgets survive initialization and settle as ordinary world pickups");
  });
}

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
  async (t) => {
    await using game = await launchDeveloperGame(t, { mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    const wrenchesBefore = await count(game, "Wrench");
    const modulesBefore = await count(game, "EXP");
    const nanitesBefore = await count(game, "Nanites");

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).paused, true);

    // Root -> Developer (the separate Developer entry) -> Cheats.
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

    // "Max out stats" provisions the sheet through the same path the debug
    // HTTP API uses, so every stat, skill and the psi tier come back at cap.
    const before = (await game.info()).player.stats;
    assert.ok(before, "the player should expose a character sheet");
    assert.ok(
      before.strength < 6 || before.skills.hack < 6,
      "a fresh medsci1 character should not already be maxed",
    );
    const after = await clickCheatAndResume(
      game,
      MAX_STATS,
      async () => (await game.info()).player.stats,
    );
    assert.ok(after, "the player should still expose a character sheet");
    for (const [name, value] of [
      ["strength", after.strength],
      ["endurance", after.endurance],
      ["agility", after.agility],
      ["psionic_ability", after.psionic_ability],
      ["cyber_affinity", after.cyber_affinity],
    ] as const) {
      assert.equal(value, 6, `${name} should be at its cap`);
    }
    for (const [name, value] of Object.entries(after.skills)) {
      assert.equal(value, 6, `skill ${name} should be at its cap`);
    }
    assert.equal(after.psi_tier, 5, "psi tier should be at its cap");
    // Modules are their own cheat - maxing the sheet must not mint currency.
    assert.equal(
      after.cyber_modules,
      before.cyber_modules,
      "maxing stats must not award cyber modules",
    );
  },
);

test(
  "exposure cheats accumulate independently and clear both while paused",
  { skip: !e2eEnabled, timeout: 120_000 },
  async (t) => {
    await using game = await launchDeveloperGame(t, { mission: "debug_minimal" });
    await game.step({ frames: 1 });
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 2 });
    await click(game, PAUSE_DEVELOPER);
    await click(game, DEV_ACTION);

    for (const [index, radiation, toxin] of [
      [7, 10, 0],
      [7, 20, 0],
      [8, 20, 10],
      [8, 20, 20],
      [9, 0, 0],
      [9, 0, 0],
      [8, 0, 10],
      [7, 10, 10],
    ]) {
      await click(game, row(index));
      const info = await game.info();
      assert.equal(info.paused, true);
      assert.equal(info.player.radiation_level, radiation);
      assert.equal(info.player.toxin_level, toxin);
    }
  },
);
