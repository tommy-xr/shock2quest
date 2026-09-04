import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";

/** Cycle the psi power selection until `name` is selected (bounded). */
export async function selectPsiPower(game: GameServer, name: string): Promise<void> {
  for (let i = 0; i < 40; i++) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
  }
  assert.fail(`could not cycle the psi power selection to ${name}`);
}
