import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { SceneObjectSummary } from "../src/types.js";
import { menuEntry, norm, vrClickCanvasPoint } from "./helpers/frontend-menu.js";

// The Developer screen: the shared dev-params row panel, hosted by a frontend
// scene reached from the main menu's repurposed Options slot, and by a second
// page of the pause overlay. These tests drive it exactly as a player would -
// pointer clicks flat, a controller ray in VR - and assert against the
// registry (`GET /v1/dev-params`) and the renderer (`/v1/scene`), so a click
// on `>` is proven to move both the value and the live VR panel.
//
// Negative-first: against the PR1 build (registry + HTTP only, no screen),
// the Options slot is inert - the first assertion of each test (the swap to
// the "developer" scene / the page turn) fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The repurposed Options slot, third of the six main-menu entries. */
const DEVELOPER_ENTRY = menuEntry(2);

// The shared panel geometry (ui/dev_params_panel.rs): rows in the GAMELOD
// pane starting at y=54 on a 28px pitch (24px tall), arrows 20px wide ending
// 8px inside the pane's right edge at x=463, "Done" on the GAMELODR button
// art. Row 0 is `panel_distance` (declaration order of the registry).
//
// The list now scrolls, and its rocker takes a 26px gutter off the pane's
// right edge whenever there is something to scroll - which shifts every row's
// arrows left by that much. Written as its own term rather than folded into
// the numbers, so the next person can see why these are not simply the pane
// edge minus the inset.
const SCROLL_GUTTER = 26;
const ROW0_INCREMENT: [number, number] = [463 - 8 - SCROLL_GUTTER - 10, 54 + 12];
const ROW0_DECREMENT: [number, number] = [
  463 - 8 - SCROLL_GUTTER - 20 - 48 - 10,
  54 + 12,
];
const DONE: [number, number] = [527 + 95 / 2, 405 + 62 / 2];

// The upper framed button (`GAMELODR.BIN` rect 2 - the load screen's "Load"
// frame): the debug-scene launcher's door on the parameters page, and the
// launch itself on the launcher page.
const ACTION: [number, number] = [527 + 96 / 2, 161 + 62 / 2];
/**
 * The launcher's tabs ride the header line (GAMELODR.BIN rect 0: 261,31
 * 202x20), split in half: Missions on the left, Debug Scenes on the right.
 * The launcher opens on Missions.
 */
const TAB_MISSIONS: [number, number] = [261 + 202 / 4, 41];
const TAB_DEBUG_SCENES: [number, number] = [261 + (3 * 202) / 4, 41];
/**
 * A row of the launcher's list: 19px rows from the pane top (y=54), x well
 * inside the pane and clear of the scroll gutter. On the Debug Scenes tab
 * row 2 is `debug_minimal` - the cheapest scene to actually start. On the
 * Missions tab (the sorted *.mis files of the canonical install) row 9 is
 * `medsci1.mis`.
 */
const sceneRow = (index: number): [number, number] => [330, 54 + index * 19 + 9];
const DEBUG_MINIMAL_ROW = 2;
const MEDSCI1_ROW = 9;

/** SIMR.BIN pause entries: five 179x76 buttons at x=400, top 20, 92px pitch. */
const pauseEntry = (index: number): [number, number] =>
  norm(400 + 179 / 2, 20 + index * 92 + 76 / 2);
const PAUSE_DEVELOPER = pauseEntry(3);
const PAUSE_CONTINUE = pauseEntry(0);

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

async function paramValue(game: GameServer, key: string): Promise<number> {
  const { params } = await game.devParams.list();
  const param = params.find((p) => p.key === key);
  assert.ok(param, `${key} must be registered`);
  return param.value;
}

test(
  "the flat Developer screen steps a param with < and > and leaves with Done",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 10 });

    // The repurposed Options slot now swaps to the Developer scene.
    await game.input.set("pointer.position", DEVELOPER_ENTRY);
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "developer");

    // Row 0 is panel_distance at its 2.0 default; `>` steps by the declared
    // 0.1, `<` steps back down.
    assert.ok(Math.abs((await paramValue(game, "panel_distance")) - 2.0) < 1e-4);
    await click(game, ROW0_INCREMENT);
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - 2.1) < 1e-4,
      "> must step the value up by the param's declared step",
    );

    // A held press is one click: keep it pressed and nothing more happens.
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 30 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 3 });
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - 2.2) < 1e-4,
      "the press edge steps once; holding must not auto-repeat",
    );

    await click(game, ROW0_DECREMENT);
    await click(game, ROW0_DECREMENT);
    assert.ok(Math.abs((await paramValue(game, "panel_distance")) - 2.0) < 1e-4);

    await game.screenshot("dev-menu-flat.png");

    // Done returns to the main menu.
    await click(game, DONE);
    assert.equal((await game.info()).mission, "main_menu");
  },
);

/**
 * Gaze-axis depth of the VR frontend panel from the renderer's own scene
 * list (the PR1 measurement): the default VR head looks along -X, so the
 * panel's canvas objects sit at x = -distance. Pointer visuals are tagged
 * (`source`) and filtered out.
 */
function panelDepth(objects: SceneObjectSummary[]): number {
  const depths = objects
    .filter((o) => o.source === null)
    .map((o) => -o.position[0])
    .sort((a, b) => a - b);
  assert.ok(depths.length > 0, "expected untagged panel objects in the scene");
  return depths[Math.floor(depths.length / 2)];
}

test(
  "the VR Developer screen's > visibly moves the live panel",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    // Into the Developer screen via the controller ray.
    await vrClickCanvasPoint(game, [400 + 179 / 2, 20 + 2 * 76 + 30]);
    assert.equal((await game.info()).mission, "developer");
    await game.step({ frames: 5 });

    const before = panelDepth((await game.scene.objects()).objects);
    assert.ok(
      Math.abs(before - 2.0) < 0.05,
      `the developer panel should hang at the 2.0 default, got ${before}`,
    );
    await game.screenshot("dev-menu-vr-2.0.png");

    // One click on panel_distance's `>`: the registry moves AND the very
    // panel being pointed at re-renders farther away.
    await vrClickCanvasPoint(game, ROW0_INCREMENT);
    assert.ok(Math.abs((await paramValue(game, "panel_distance")) - 2.1) < 1e-4);
    await game.step({ frames: 2 });
    const after = panelDepth((await game.scene.objects()).objects);
    assert.ok(
      Math.abs(after - 2.1) < 0.05,
      `the panel should re-render at the tuned 2.1, got ${after}`,
    );

    // Step back down; the panel comes home.
    await vrClickCanvasPoint(game, ROW0_DECREMENT);
    await game.step({ frames: 2 });
    const restored = panelDepth((await game.scene.objects()).objects);
    assert.ok(Math.abs(restored - 2.0) < 0.05, `expected 2.0, got ${restored}`);

    // Done returns to the main menu.
    await vrClickCanvasPoint(game, DONE);
    assert.equal((await game.info()).mission, "main_menu");
  },
);

test(
  "the pause overlay's Developer page tunes without leaving the mission",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).paused, true);

    // The repurposed Options slot turns the overlay's page - the mission
    // stays loaded and paused underneath.
    await game.input.set("pointer.position", PAUSE_DEVELOPER);
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 3 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 3 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).paused, true, "the page turn must not resume");
    assert.equal((await game.info()).mission, "medsci1.mis", "no scene swap");

    // The same shared rows at the same canvas coordinates as the frontend
    // host: one page description, two hosts.
    await click(game, ROW0_INCREMENT);
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - 2.1) < 1e-4,
      "> on the pause page must step the same registry",
    );
    await game.screenshot("dev-menu-pause.png");
    await click(game, ROW0_DECREMENT);

    // Done returns to the root page (still paused), where Continue resumes.
    await click(game, DONE);
    assert.equal((await game.info()).paused, true, "Done turns the page, not the sim");
    await game.input.set("pointer.position", PAUSE_CONTINUE);
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 3 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 3 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).paused, false, "Continue on the root resumes");
  },
);

// The debug-scene launcher (`scenes/developer.rs`). On a headset the runtime
// picks its scene from a file read at startup, so without this page a change
// of debug scene - or a death inside one - costs an APK relaunch. Both
// presentations drive the same page through the same hit test.
//
// Negative-first: before the launcher existed the upper framed button was
// inert, so the first assertion of each test (the page turn away from the
// parameter rows) fails - the screen stays on the rows.

test(
  "the flat Developer screen launches a debug scene and can back out of the list",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 10 });
    await click(game, [489, 202]);
    assert.equal((await game.info()).mission, "developer");

    // The upper framed button turns the page - it does not swap the scene.
    await click(game, ACTION);
    assert.equal((await game.info()).mission, "developer");
    await game.screenshot("dev-scenes-flat.png");

    // "Done" on the launcher goes back to the parameters, not to the menu...
    await click(game, DONE);
    assert.equal((await game.info()).mission, "developer");
    // ...and the parameter rows really are back: `>` steps a value again.
    const before = await paramValue(game, "panel_distance");
    await click(game, ROW0_INCREMENT);
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - (before + 0.1)) < 1e-4,
      "Done on the launcher must return to the parameter rows",
    );
    await click(game, ROW0_DECREMENT);

    // Select a scene on the Debug Scenes tab and launch it.
    await click(game, ACTION);
    await click(game, TAB_DEBUG_SCENES);
    await click(game, sceneRow(DEBUG_MINIMAL_ROW));
    await click(game, ACTION);
    assert.equal(
      (await game.info()).mission,
      "debug_minimal",
      "Launch must start the selected scene",
    );
  },
);

test(
  "the flat Developer screen launches a full mission from the Missions tab",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 10 });
    await click(game, [489, 202]);
    assert.equal((await game.info()).mission, "developer");

    // The launcher opens on the Missions tab; tab over to Debug Scenes and
    // back, so the round trip is exercised end to end, then pick a row.
    await click(game, ACTION);
    await click(game, TAB_DEBUG_SCENES);
    await click(game, TAB_MISSIONS);
    await click(game, sceneRow(MEDSCI1_ROW));
    await click(game, ACTION);
    // Row 9 is medsci1.mis on the canonical 23-mission install (the same
    // assumption missions.e2e.test.ts hardcodes). Asserting the exact name
    // proves the row CLICK picked the mission - the preselected row 0 would
    // boot command1.mis, so a looser ".mis booted" check could pass without
    // the selection ever moving.
    assert.equal(
      (await game.info()).mission,
      "medsci1.mis",
      "Launch on the Missions tab must boot the clicked row's mission (canonical install assumed)",
    );

    // ...and it is a real world, not just a scene-name swap.
    const { entities } = await game.entities.list({ limit: 200 });
    assert.ok(entities.length > 100, `expected a populated mission, got ${entities.length}`);
  },
);

test(
  "the VR Developer screen launches a debug scene through the controller ray",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });
    await vrClickCanvasPoint(game, [400 + 179 / 2, 20 + 2 * 76 + 30]);
    assert.equal((await game.info()).mission, "developer");

    // The same canvas points as the flat run, reached by the ray.
    await vrClickCanvasPoint(game, ACTION);
    assert.equal((await game.info()).mission, "developer");
    await game.step({ frames: 5 });
    await game.screenshot("dev-scenes-vr.png");

    await vrClickCanvasPoint(game, TAB_DEBUG_SCENES);
    await vrClickCanvasPoint(game, sceneRow(DEBUG_MINIMAL_ROW));
    await vrClickCanvasPoint(game, ACTION);
    assert.equal((await game.info()).mission, "debug_minimal");
  },
);
