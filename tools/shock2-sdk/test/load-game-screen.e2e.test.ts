import assert from "node:assert/strict";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { AIM_AT_PANEL, panelPoint } from "./helpers/frontend-menu.js";

// End-to-end test for the main menu -> load-game screen -> back round trip,
// and for actually restoring a save from the list.
//
// These are frontend scenes, so there is nothing in the world to assert on
// while a menu is up: entity count is the observable that separates "a menu is
// showing" (0 entities) from "a mission is live" (thousands).
//
// Negative-first: before the load screen existed, clicking "Load Game" did
// nothing at all, so the entity count stayed 0 through the whole sequence and
// `loading a save from the list restores a mission` failed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Canvas-space widget centers on the 640x480 UI canvas, normalized to the
// [0,1] pointer channel. From the shipped layouts: MAINR.BIN button 1 is
// "Load Game" at (400,96,179x60); GAMELODR.BIN gives Load at (527,161,96x62),
// Done at (527,405,95x62), and a list starting at (261,54) with a 19px pitch.
const CANVAS_W = 640;
const CANVAS_H = 480;
const norm = (x: number, y: number): [number, number] => [x / CANVAS_W, y / CANVAS_H];

const LOAD_GAME_ENTRY_CANVAS: [number, number] = [400 + 179 / 2, 96 + 60 / 2];
const LOAD_BUTTON_CANVAS: [number, number] = [527 + 96 / 2, 161 + 62 / 2];
const DONE_BUTTON_CANVAS: [number, number] = [527 + 95 / 2, 405 + 62 / 2];
const FIRST_ROW_CANVAS: [number, number] = [261 + 202 / 2, 54 + 19 / 2];
const LOAD_GAME_ENTRY = norm(...LOAD_GAME_ENTRY_CANVAS);
const LOAD_BUTTON = norm(...LOAD_BUTTON_CANVAS);
const DONE_BUTTON = norm(...DONE_BUTTON_CANVAS);
const FIRST_ROW = norm(...FIRST_ROW_CANVAS);

async function click(game: GameServer, [x, y]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", [x, y]);
  await game.step({ frames: 2 });
  // The screens act on a rising press edge, so the click must be a pulse.
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 6 });
}

async function vrClick(game: GameServer, point: [number, number]): Promise<void> {
  const [, y, z] = panelPoint(norm(...point));
  await game.input.set("right_hand.rotation", AIM_AT_PANEL);
  await game.input.set("right_hand.position", [0, y, z]);
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 3 });
}

async function entityCount(game: GameServer): Promise<number> {
  const result = await game.entities.list();
  return result.entities.length;
}

/**
 * Guarantee the load screen has something to list. The screen reads the real
 * save directory, so a machine that has never saved would otherwise show
 * "< EMPTY >" and the load assertions would be vacuous.
 */
async function seedSave(): Promise<void> {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 30 });
  await game.input.trigger("QuickSave");
  await game.step({ frames: 30 });
}

test(
  "the load screen opens from the menu and Done returns to it",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await seedSave();
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 5 });

    // A frontend scene has no world entities.
    assert.equal(await entityCount(game), 0, "the menu should have no world");

    await click(game, LOAD_GAME_ENTRY);
    // Still a frontend scene - but a different one. Clicking "Done" only gets
    // back to a menu if we actually left it, which the load assertion below
    // pins down properly.
    assert.equal(await entityCount(game), 0);

    await click(game, DONE_BUTTON);
    assert.equal(await entityCount(game), 0, "Done should land on the menu");

    // Proof we are on the main menu and not still on the load screen: the
    // load screen has no widget at the "Load Game" entry's position, whereas
    // the menu does - so clicking there opens the load screen again, and from
    // there a save can be loaded.
    await click(game, LOAD_GAME_ENTRY);
    await click(game, FIRST_ROW);
    await click(game, LOAD_BUTTON);
    await game.step({ frames: 30 });
    assert.ok(
      (await entityCount(game)) > 0,
      "the menu -> load screen path should still work after a round trip",
    );
  },
);

test(
  "loading a save from the list restores a mission",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await seedSave();
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 5 });
    assert.equal(await entityCount(game), 0);

    await click(game, LOAD_GAME_ENTRY);
    // Row 0 is the most recent save; the screen preselects it, but click it
    // anyway so the test covers row selection rather than the default.
    await click(game, FIRST_ROW);
    await click(game, LOAD_BUTTON);
    await game.step({ frames: 30 });

    assert.ok(
      (await entityCount(game)) > 100,
      "loading a save should bring up a populated mission",
    );
  },
);

test(
  "a corrupt save reports Load Failed on the shared flat and VR canvas",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async (t) => {
    const assetRoot = process.env.DARK_ASSET_PATH;
    assert.ok(assetRoot, "DARK_ASSET_PATH must explicitly select the 25AE root");
    const saveName = `issue929_corrupt_${Date.now()}`;
    const savePath = join(assetRoot, "saves", `${saveName}.sav`);
    mkdirSync(join(assetRoot, "saves"), { recursive: true });
    writeFileSync(savePath, "this is not a valid save file");
    t.after(() => rmSync(savePath, { force: true }));

    const captures = join(tmpdir(), `shock2quest-issue929-${process.pid}`);
    mkdirSync(captures, { recursive: true });

    let flatFailureObjects = 0;
    {
      await using game = await GameServer.launch({ mission: "main_menu" });
      await game.step({ frames: 5 });
      await click(game, LOAD_GAME_ENTRY);
      assert.equal((await game.info()).mission, "load_game");

      await game.input.set("pointer.position", norm(20, 20));
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 3 });
      const before = await game.screenshot(join(captures, "flat-before.png"));

      await click(game, FIRST_ROW);
      await click(game, LOAD_BUTTON);
      await game.input.set("pointer.position", norm(20, 20));
      await game.step({ frames: 3 });
      assert.equal(
        (await game.info()).mission,
        "load_game",
        "a malformed save must leave the responsive load screen active",
      );
      const after = await game.screenshot(join(captures, "flat-failed.png"));
      assert.equal(
        readFileSync(before.full_path).equals(readFileSync(after.full_path)),
        false,
        "the failure result must visibly change the canvas",
      );
      const flatObjects = (await game.scene.objects()).objects.filter(
        (object) => object.source === null,
      );
      flatFailureObjects = flatObjects.length;
      assert.ok(flatFailureObjects > 0);
      assert.ok(
        flatObjects.every((object) => object.position.every((axis) => axis === 0)),
        "the flat failure canvas must stay in screen space",
      );

      // Retry remains a rising-edge action and reports the same failure
      // without wedging the runtime.
      await click(game, LOAD_BUTTON);
      assert.equal((await game.info()).mission, "load_game");

      // Leaving discards the transient status; re-entry starts at the shipped
      // initial prompt again.
      await click(game, DONE_BUTTON);
      await click(game, LOAD_GAME_ENTRY);
      await game.input.set("pointer.position", norm(20, 20));
      await game.step({ frames: 3 });
      const reentered = await game.screenshot(join(captures, "flat-reentered.png"));
      assert.ok(
        readFileSync(before.full_path).equals(readFileSync(reentered.full_path)),
        "a fresh load screen must restore the initial status",
      );
    }

    {
      await using game = await GameServer.launch({
        mission: "main_menu",
        debugFlags: ["--vr"],
      });
      await game.step({ frames: 5 });
      await vrClick(game, LOAD_GAME_ENTRY_CANVAS);
      assert.equal((await game.info()).mission, "load_game");
      await vrClick(game, FIRST_ROW_CANVAS);
      await vrClick(game, LOAD_BUTTON_CANVAS);

      // Remove the pointer visuals so object-count parity compares only the
      // canvas built once by LoadGameScene.
      await game.input.set("right_hand.rotation", AIM_AT_PANEL);
      await game.input.set("right_hand.position", [0, 10, 10]);
      await game.input.set("right_hand.trigger", 0);
      await game.input.set("left_hand.rotation", AIM_AT_PANEL);
      await game.input.set("left_hand.position", [0, 10, 10]);
      await game.input.set("left_hand.trigger", 0);
      await game.step({ frames: 3 });
      assert.equal((await game.info()).mission, "load_game");
      await game.screenshot(join(captures, "vr-failed.png"));
      const vrObjects = (await game.scene.objects()).objects.filter(
        (object) => object.source === null,
      );
      assert.equal(
        vrObjects.length,
        flatFailureObjects,
        "flat and VR must present the same failure canvas contents",
      );
      assert.ok(
        vrObjects.every((object) => Math.hypot(...object.position) > 0.5),
        "the VR failure canvas must stay on its world panel",
      );
    }
  },
);
