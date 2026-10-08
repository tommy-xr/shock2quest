import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";

for (const vr of [false, true]) {
  test(`Earth cyber interface: missing software and authored installation (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "earth.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 3 });
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 30 });
      const missing = await game.ui.state();
      assert.equal(missing.mode, "use");
      assert.equal(missing.strip, null);
      assert.equal(missing.active_panel, null);
      assert.deepEqual(missing.readout.map(e => e.label), ["system_menu"]);
      assert.equal(missing.utilities.length, 0);
      assert.ok(missing.messages.includes("Cyber Interface software not installed."));

      const menu = missing.readout[0]!;
      if (vr) {
        await clickCanvasWithRay(game, requirePanelPose(missing), canvasCenter(menu));
      } else {
        await clickUiElement(game, menu);
      }
      assert.equal((await game.info()).paused, true);
      await game.input.trigger("TogglePauseMenu");
      await game.step({ frames: 2 });
      if ((await game.ui.state()).mode === "use") {
        await game.input.trigger("ToggleUseMode");
        await game.step({ frames: 2 });
      }
      // The real entrance tripwire enables the HUD via its teleport trap's
      // ChangeInterface script, not a test-only quest flag override.
      await crossEarthTrainingTripwire(game, 313, 319);
      await game.input.trigger("ToggleUseMode");
      await game.step({ frames: 30 });
      const installed = await game.ui.state();
      assert.ok(installed.strip, JSON.stringify(installed));
      assert.ok(installed.readout.length > 1);
      assert.ok(!installed.messages.includes("Cyber Interface software not installed."));

      // Authored ChangeInterface also supports removing the interface.
      const [marker] = await game.entities.byTemplate(328);
      assert.ok(marker);
      await game.entities.sendMessage(marker.id, { type: "TurnOff" });
      await game.step({ frames: 3 });
      const disabled = await game.ui.state();
      assert.equal(disabled.strip, null);
      assert.deepEqual(disabled.readout.map(e => e.label), ["system_menu"]);
      assert.ok(disabled.messages.includes("Cyber Interface software not installed."));
    });
}
