import type { GameServer } from "../../src/index.js";

/** Fire one round: edge-triggered pull (fires on the rising edge), then release,
 * stepping a frame for each so the next pull is a fresh edge. */
export async function fireOnce(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 1.0);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0.0);
  await game.step({ frames: 1 });
}
