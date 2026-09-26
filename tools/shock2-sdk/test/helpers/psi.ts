import type { GameServer } from "../../src/index.js";

/** Cycle the psi power selection until `name` is the selected power. */
export async function selectPsiPower(game: GameServer, name: string): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  throw new Error(`psi power ${name} was never selected`);
}
