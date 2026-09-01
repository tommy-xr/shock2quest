import type { GameServer } from "../../src/index.js";
import { clickCanvasPoint } from "./frontend-menu.js";

// A training tour ends on the debrief screen (DEBRIEF.PCX), which waits for the
// player before the departure runs. Any test that drives a tour therefore has
// to dismiss it, the way a player does.

/** The Continue button's center, from DEBRIEFR.BIN (425,401 210x74). */
export const DEBRIEF_CONTINUE: [number, number] = [425 + 210 / 2, 401 + 74 / 2];

/** Whether the debrief screen is what is on screen right now. */
export async function debriefIsUp(game: GameServer): Promise<boolean> {
  return (await game.info()).mission === "debrief";
}

/**
 * Click Continue on the debrief page if it is up, as a real rising edge, and
 * report whether there was one to dismiss.
 */
export async function dismissDebrief(game: GameServer): Promise<boolean> {
  if (!(await debriefIsUp(game))) return false;
  await clickCanvasPoint(game, DEBRIEF_CONTINUE);
  return true;
}
