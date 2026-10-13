import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { clickWorldPanelElement } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`Tic-Tac-Triop plays against Xerxes (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      if (vr) await game.input.set("left_hand.squeeze", 1);
      const handheld = await game.player.spawnItem("Gameboy", vr ? { hand: "left" } : undefined);
      await game.step({ frames: 2 });
      const cart = await game.player.spawnItem("TTT Cart");
      await game.entities.sendMessage(cart.entity_id, { type: "Frob" });
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      const click: typeof clickUiElement = vr
        ? (g, element) => clickWorldPanelElement(g, [188, 296, 0], 1, element)
        : clickUiElement;
      let ui = await game.ui.state();
      const select = ui.active_panel?.elements.find(e => e.label === "game-8");
      assert.ok(select, "installed Tic-Tac-Triop must be selectable");
      await click(game, select);
      ui = await game.ui.state();
      assert.ok(ui.active_panel?.elements.some(e => e.label === "ttt-cell-4"), "selecting opens the original 3x3 board");
      let playerMoves = 0;
      for (let turn = 0; turn < 5; turn++) {
        ui = await game.ui.state();
        const empty = ui.active_panel?.elements.find(e => e.label?.startsWith("ttt-cell-"));
        if (!empty) break;
        await click(game, empty);
        playerMoves++;
        ui = await game.ui.state();
        const playerMarks = ui.active_panel!.elements.filter(e => e.texture === "tttplayr.pcx");
        const aiMarks = ui.active_panel!.elements.filter(e => e.texture === "tttshodn.pcx");
        assert.equal(playerMarks.length, playerMoves, "one press places one player mark");
        assert.ok(aiMarks.length >= playerMoves || !ui.active_panel!.elements.some(e => e.label?.startsWith("ttt-cell-")), "Xerxes responds unless game ended");
        if (!ui.active_panel!.elements.some(e => e.label?.startsWith("ttt-cell-"))) break;
        await click(game, empty);
        const afterRepeat = await game.ui.state();
        assert.equal(afterRepeat.active_panel!.elements.filter(e => e.texture === "tttplayr.pcx").length, playerMoves, "occupied square cannot be replayed");
      }
      ui = await game.ui.state();
      assert.ok(playerMoves >= 2);
      const restart = ui.active_panel!.elements.find(e => e.label === "ttt-restart");
      assert.ok(restart, "finished game offers restart");
      await click(game, restart);
      ui = await game.ui.state();
      assert.ok(ui.active_panel!.elements.filter(e => e.label?.startsWith("ttt-cell-")).length >= 8);
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-8"), "reopening returns to the index");
    });
}
