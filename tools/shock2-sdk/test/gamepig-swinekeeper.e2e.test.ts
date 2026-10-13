import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";

for (const vr of [false, true]) {
  test(`Swinekeeper flags, reveals, loses, and restarts (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      const { click } = await openGamePig(game, vr, "Swinekeeper");
      const press = async (label: string) => {
        const ui = await game.ui.state();
        const element = ui.active_panel?.elements.find(e => e.label === label);
        assert.ok(element, `missing playable control ${label}`);
        await click(game, element);
      };
      await press("game-3");
      let elements = (await game.ui.state()).active_panel!.elements;
      assert.equal(elements.filter(e => e.texture === "swineune.pcx").length, 60);
      await press("swine-flag-mode");
      await press("swine-cell-0");
      elements = (await game.ui.state()).active_panel!.elements;
      assert.equal(elements.filter(e => e.texture === "swineflg.pcx").length, 1, "placing a flag returns to reveal mode");
      assert.ok(elements.some(e => e.text === "11"));
      await press("swine-cell-0");
      assert.equal((await game.ui.state()).active_panel!.elements.filter(e => e.texture === "swineune.pcx").length, 59, "normal reveal ignores a flag");
      await press("swine-flag-mode");
      await press("swine-cell-0");
      elements = (await game.ui.state()).active_panel!.elements;
      assert.equal(elements.filter(e => e.texture === "swineune.pcx").length, 60, "flag removal restores a hidden tile");
      assert.ok(elements.some(e => e.text === "12"));
      await press("swine-flag-mode");
      for (let i = 0; i < 60; i++) {
        if ((await game.ui.state()).active_panel!.elements.some(e => e.label === "swine-restart")) break;
        await press(`swine-cell-${i}`);
      }
      elements = (await game.ui.state()).active_panel!.elements;
      assert.equal(elements.filter(e => e.texture === "swinecrn.pcx").length, 12, "loss exposes every corn tile");
      assert.ok(elements.some(e => e.text?.includes("corn")));
      assert.ok(elements.every(e => !e.text?.includes("\\n")), "authored line breaks must render as separate lines");
      await press("swine-restart");
      elements = (await game.ui.state()).active_panel!.elements;
      assert.equal(elements.filter(e => e.texture === "swineune.pcx").length, 60);
      assert.ok(elements.some(e => e.text === "12"));
    });
}
