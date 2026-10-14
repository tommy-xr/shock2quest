import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  for (const mission of ["earth.mis", "medsci1.mis", "ops2.mis", "earth_horde_test", "debug_interactions"]) {
    test(`cyber interface at startup: ${mission} (${vr ? "VR" : "flat"})`,
      { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
        await using game = await GameServer.launch({ mission, debugFlags: vr ? ["--vr"] : [] });
        await game.step({ frames: 3 });
        await game.input.trigger("ToggleUseMode");
        await game.step({ frames: 30 });
        const ui = await game.ui.state();
        const installed = mission !== "earth.mis";
        assert.equal(ui.mode, "use");
        assert.equal(ui.strip !== null, installed, "inventory availability matches interface installation");
        assert.equal(ui.utilities.length > 0, installed, "utility buttons match interface installation");
        assert.equal(ui.messages.includes("Cyber Interface software not installed."), !installed);
      });
  }
}
