import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { SceneObjectSummary } from "../src/types.js";
import {
  DEV_ACTION,
  DEV_DONE,
  clickCanvas as click,
  norm,
  pauseEntry,
  vrClickCanvasPoint,
} from "./helpers/frontend-menu.js";

// The Developer screen: the shared dev-params row panel, hosted by a frontend
// scene reached from the main menu's dedicated Developer button, and by a second
// page of the pause overlay. These tests drive it exactly as a player would -
// pointer clicks flat, a controller ray in VR - and assert against the
// registry (`GET /v1/dev-params`) and the renderer (`/v1/scene`), so a click
// on `>` is proven to move both the value and the live VR panel.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The dedicated bottom-left Developer button. */
const DEVELOPER_BUTTON: [number, number] = [88, 444];
const DEVELOPER_ENTRY = norm(...DEVELOPER_BUTTON);

// Camera & view is root category row 5. Its first parameter is
// panel_distance. Compact single-line entries fit without a scroll gutter.
const CAMERA_CATEGORY: [number, number] = [330, 54 + 5 * 28 + 12];
const ROW0_INCREMENT: [number, number] = [449, 66];
const ROW0_DECREMENT: [number, number] = [407, 66];

// The upper framed button (`GAMELODR.BIN` rect 2 - the load screen's "Load"
// frame): the debug-scene launcher's door on the parameters page, and the
// launch itself on the launcher page.
/**
 * The launcher's tabs ride the header line (GAMELODR.BIN rect 0: 261,31
 * 202x20), split in half: Missions on the left, Debug Scenes on the right.
 * The launcher opens on Missions.
 */
const TAB_MISSIONS: [number, number] = [261 + 202 / 4, 41];
// Three tabs split the header (Missions / Debug Scenes / Video); aim at the
// middle third's center.
const TAB_DEBUG_SCENES: [number, number] = [261 + (3 * 202) / 6, 41];
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

const PAUSE_DEVELOPER = pauseEntry(3);

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

    // The dedicated Developer button opens the Developer scene.
    await game.input.set("pointer.position", DEVELOPER_ENTRY);
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "developer");

    await click(game, CAMERA_CATEGORY);

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
    await click(game, DEV_DONE);
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

/** Pull the trigger over a canvas point on the VR panel, as a rising edge. */


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
    await vrClickCanvasPoint(game, DEVELOPER_BUTTON);
    assert.equal((await game.info()).mission, "developer");
    await game.step({ frames: 5 });

    await vrClickCanvasPoint(game, CAMERA_CATEGORY);

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
    await vrClickCanvasPoint(game, DEV_DONE);
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
    await click(game, PAUSE_DEVELOPER);
    assert.equal((await game.info()).paused, true, "the page turn must not resume");
    assert.equal((await game.info()).mission, "medsci1.mis", "no scene swap");

    await click(game, CAMERA_CATEGORY);

    // The same shared rows at the same canvas coordinates as the frontend
    // host: one page description, two hosts.
    await click(game, ROW0_INCREMENT);
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - 2.1) < 1e-4,
      "> on the pause page must step the same registry",
    );
    await game.screenshot("dev-menu-pause.png");
    await click(game, ROW0_DECREMENT);

    // Resume closes directly. Developer restores the last submenu on re-entry.
    await click(game, DEV_DONE);
    assert.equal((await game.info()).paused, false);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 5 });
    await click(game, PAUSE_DEVELOPER);
    await click(game, ROW0_INCREMENT);
    assert.ok(Math.abs((await paramValue(game, "panel_distance")) - 2.1) < 1e-4);
    await click(game, DEV_DONE);
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
    await click(game, DEVELOPER_BUTTON);
    assert.equal((await game.info()).mission, "developer");

    // The upper framed button turns the page - it does not swap the scene.
    await click(game, DEV_ACTION);
    assert.equal((await game.info()).mission, "developer");
    await game.screenshot("dev-scenes-flat.png");

    // "Done" on the launcher goes back to the parameters, not to the menu...
    await click(game, DEV_DONE);
    assert.equal((await game.info()).mission, "developer");
    await click(game, CAMERA_CATEGORY);
    // ...and the parameter rows really are back: `>` steps a value again.
    const before = await paramValue(game, "panel_distance");
    await click(game, ROW0_INCREMENT);
    assert.ok(
      Math.abs((await paramValue(game, "panel_distance")) - (before + 0.1)) < 1e-4,
      "Done on the launcher must return to the parameter rows",
    );

    await click(game, ROW0_DECREMENT);

    // Select a scene on the Debug Scenes tab and launch it.
    await click(game, DEV_ACTION);
    await click(game, TAB_DEBUG_SCENES);
    await click(game, sceneRow(DEBUG_MINIMAL_ROW));
    await click(game, DEV_ACTION);
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
    await click(game, DEVELOPER_BUTTON);
    assert.equal((await game.info()).mission, "developer");

    // The launcher opens on the Missions tab; tab over to Debug Scenes and
    // back, so the round trip is exercised end to end, then pick a row.
    await click(game, DEV_ACTION);
    await click(game, TAB_DEBUG_SCENES);
    await click(game, TAB_MISSIONS);
    await click(game, sceneRow(MEDSCI1_ROW));
    await click(game, DEV_ACTION);
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
    await vrClickCanvasPoint(game, DEVELOPER_BUTTON);
    assert.equal((await game.info()).mission, "developer");

    // The same canvas points as the flat run, reached by the ray.
    await vrClickCanvasPoint(game, DEV_ACTION);
    assert.equal((await game.info()).mission, "developer");
    await game.step({ frames: 5 });
    await game.screenshot("dev-scenes-vr.png");

    await vrClickCanvasPoint(game, TAB_DEBUG_SCENES);
    await vrClickCanvasPoint(game, sceneRow(DEBUG_MINIMAL_ROW));
    await vrClickCanvasPoint(game, DEV_ACTION);
    assert.equal((await game.info()).mission, "debug_minimal");
  },
);

for (const vr of [false, true]) {
  test(
    `${vr ? "VR" : "flat"} categories preserve location and keep Locked editable`,
    { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 300_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: "main_menu",
        debugFlags: vr ? ["--vr"] : [],
      });
      const press = (point: [number, number]) =>
        vr ? vrClickCanvasPoint(game, point) : click(game, point);
      const category = (index: number): [number, number] => [330, 54 + index * 28 + 12];
      const back: [number, number] = [309, 436];
      const zoneKeys = [
        "melee_glove_overlay", "vr_backpack_zones", "vr_glove_spheres",
        "vr_ammo_pouch_zones", "vr_holster_zones", "vr_support_grips", "clip_zone",
      ];
      await game.step({ frames: 10 });
      await press(DEVELOPER_BUTTON);
      await press(category(0)); // Visualizations
      await press(category(1)); // Hands & zones (after the bulk row)
      await press([310, 74]); // All on
      for (const key of zoneKeys) assert.equal(await paramValue(game, key), 1, key);
      assert.equal(await paramValue(game, "free_camera"), 0);
      assert.equal(await paramValue(game, "glove_fit_visible"), 1);
      await press([410, 74]); // All off
      for (const key of zoneKeys) assert.equal(await paramValue(game, key), 0, key);

      // Individual changes address the displayed row, and re-entry restores
      // the same submenu. Nonzero scroll is covered with a short pane in Rust.
      await press([449, 122]);
      assert.equal(await paramValue(game, "vr_backpack_zones"), 1);
      assert.equal(await paramValue(game, "melee_glove_overlay"), 0);
      await press(DEV_DONE);
      await press(DEVELOPER_BUTTON);
      await press([449, 122]);
      assert.equal(await paramValue(game, "vr_backpack_zones"), 0);
      assert.equal(await paramValue(game, "melee_glove_overlay"), 0);

      await press(back);
      await press(back);
      await press(category(8)); // Locked
      await press(category(0)); // Hands & gloves
      const locked = (await game.devParams.list()).params.find(p => p.key === "glove_forward_cm");
      assert.equal(locked?.locked, true);
      await press([449, 66]);
      assert.equal(await paramValue(game, "glove_forward_cm"), -14.5);
      await game.devParams.set("glove_forward_cm", -15);
      assert.equal(await paramValue(game, "glove_forward_cm"), -15);
    },
  );
}
