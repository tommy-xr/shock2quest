import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";

for (const vr of [false, true]) {
  test(`cyber interface exit button closes every panel (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({
        mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 3 });
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 30 });
      let ui = await game.ui.state();
      const character = ui.utilities.find(e => e.label === "character_stats");
      assert.ok(character);
      if (vr) {
        await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(character));
      } else {
        await clickUiElement(game, character);
      }
      ui = await game.ui.state();
      assert.ok(ui.utilities.some(e => e.label === "utility_close"), "character sheet is open");
      const exit = ui.readout.find(e => e.label === "close_use_mode");
      assert.ok(exit, "the cyber interface must offer an on-screen exit");
      assert.deepEqual(exit.rect, [304, 437, 32, 40]);
      assert.ok(ui.readout_elements.some(e => e.texture === "iface/micelook.png"));
      if (vr) {
        await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(exit));
      } else {
        await clickUiElement(game, exit);
      }
      const closed = await game.ui.state();
      assert.equal(closed.mode, "shooter");
      assert.equal(closed.strip, null);
      assert.equal(closed.active_panel, null);
      assert.deepEqual(closed.utilities, []);
      assert.deepEqual(closed.readout, []);
      assert.equal((await game.info()).paused, false);
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 30 });
      assert.ok(!(await game.ui.state()).utilities.some(e => e.label === "utility_close"),
        "reopening must not restore a dismissed utility panel");
    });
}
