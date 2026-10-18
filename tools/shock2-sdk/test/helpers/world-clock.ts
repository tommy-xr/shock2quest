import type { GameServer } from "../../src/index.js";

/** World-clock seconds that pass over `frames` player frames (60 per second). */
export async function worldSeconds(game: GameServer, frames: number): Promise<number> {
  const before = (await game.step({ frames: 1 })).render_time;
  return (await game.step({ frames })).render_time - before;
}
