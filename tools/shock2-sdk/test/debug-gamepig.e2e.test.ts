import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`debug_gamepig ${vr ? "VR" : "flat"}: the handheld is on hand with every game installed`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_gamepig", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      const handheld = (await game.entities.list({ filter: "Gameboy" })).entities.find(e => e.template_id === -3864);
      assert.ok(handheld, "the scene provides the GamePig");
      await game.entities.sendMessage(handheld.id, { type: "Frob" });
      await game.step({ frames: 80 });
      const labels = (await game.ui.state()).active_panel!.elements.map(e => e.text);
      for (const name of ["Swinekeeper", "Tic-Tac-Triop", "Golf"]) {
        assert.ok(labels.includes(name), `${name} is installed`);
      }
    });
}
