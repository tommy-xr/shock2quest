import type { GameServer } from "../../src/index.js";

// The authored campaign moments play a movie before the level they lead to, so
// a test that triggers one lands on the cutscene scene first. `info().mission`
// reports the requested video name (not the file the install resolves it to),
// which is how a cutscene is told apart from a level here.

/** Whether `mission` names a cutscene rather than a level or debug scene. */
export function isCutsceneScene(mission: string): boolean {
  return /\.(avi|ogv)$/i.test(mission);
}

/**
 * Step until no cutscene is on screen, and return the videos seen in order.
 *
 * Stepping is a fixed 60 Hz, so this advances real playback time - a chunk at a
 * time rather than one huge step, because the authored clips run from ten
 * seconds to nearly three minutes and a caller should not have to know which.
 */
export async function stepPastCutscenes(
  game: GameServer,
  { timeoutSeconds = 400, chunkFrames = 120 } = {},
): Promise<string[]> {
  const seen: string[] = [];
  for (let stepped = 0; stepped <= timeoutSeconds * 60; stepped += chunkFrames) {
    const { mission } = await game.info();
    if (!isCutsceneScene(mission)) return seen;
    if (seen[seen.length - 1] !== mission) seen.push(mission);
    await game.step({ frames: chunkFrames });
  }
  throw new Error(
    `cutscene(s) ${JSON.stringify(seen)} did not finish within ${timeoutSeconds}s of stepping`,
  );
}
