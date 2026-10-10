import assert from "node:assert/strict";
import type { GameServer, UiPanel } from "../../src/index.js";
import { clickUiElement } from "./ui.js";
import { nodeOverlay } from "./hrm.js";

export function hasHackTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some((element) => element.texture?.toLowerCase() === texture);
}

/** Play the visible paid HRM board, avoiding the mines its artwork reveals. */
export async function winHack(game: GameServer, click = clickUiElement): Promise<UiPanel> {
  for (let attempt = 0; attempt < 8; attempt++) {
    let panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "the hacking panel must remain open");
    if (hasHackTexture(panel, "winh.pcx")) return panel;
    const deal = panel.elements.find((element) => element.label === "start-hack" || element.label === "reset-hack");
    assert.ok(deal, "an unfinished board must offer START/RESET");
    await click(game, deal);
    for (let y = 0; y < 4; y++) {
      for (let x = 0; x < 5; x++) {
        panel = (await game.ui.state()).active_panel;
        assert.ok(panel);
        if (hasHackTexture(panel, "winh.pcx")) return panel;
        assert.ok(!hasHackTexture(panel, "loseh.pcx"), "the visible mines must be avoided");
        assert.ok(!hasHackTexture(panel, "payh.pcx"), "the wallet must cover the paid hack");
        const node = panel.elements.find((element) => element.label === `node-${x}-${y}`);
        if (!node || nodeOverlay(panel.elements, node)) continue;
        await click(game, node);
      }
    }
  }
  assert.fail("could not win the paid HRM within eight deals");
}
