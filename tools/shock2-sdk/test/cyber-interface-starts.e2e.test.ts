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

// Builds that skipped graduation's install saved MedSci1 with Earth's hidden
// interface; loading such a save must restore the interface (#2276).
test("cyber interface: a MedSci1 save made without it loads installed (VR)",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 3 });
    await game.quests.set("HideInterface", "incomplete");
    const saveName = `cyber_interface_stuck_${Date.now()}`;
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 3 });
    assert.equal(await game.quests.get("HideInterface"), "unknown", "loading MedSci1 installs the interface");
    await game.input.trigger("MenuButton");
    await game.step({ frames: 5 });
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use");
    assert.ok(ui.strip !== null, "the inventory is available");
    assert.ok(!ui.messages.includes("Cyber Interface software not installed."));
  });
