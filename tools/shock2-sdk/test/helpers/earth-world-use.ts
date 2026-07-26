import type { GameServer } from "../../src/index.js";
import type { EntitySummary } from "../../src/types.js";

export interface EarthWorldUseStaging {
  horizontalOffset: number;
  verticalOffset: number;
  /** Fixed pitch, in degrees. Omit to aim at the item from wherever the camera
   * actually ends up, which is what you want unless you are specifically
   * testing a fixed-aim case. */
  pitchDeg?: number;
}

const DEFAULT_STAGING: EarthWorldUseStaging = {
  horizontalOffset: 0.1,
  verticalOffset: -1.6,
  // Ignored unless `pitchDeg` is given explicitly - the aim is computed from the
  // measured camera position instead. See below.
  pitchDeg: undefined,
};

/** Eye height above the player's body position, in world units:
 * `PLAYER_EYE_HEIGHT` (4.0) divided by `dark::SCALE_FACTOR` (2.5). */
const EYE_HEIGHT = 1.6;

/**
 * Aim at and world-use an authored entity through the normal flat crosshair
 * and squeeze input. Teleporting only stages the camera; acquisition still
 * flows through the game's real interaction controller.
 *
 * The close staging is intentional for Earth training: booth geometry sits
 * between ordinary standing positions and the display items.
 *
 * The aim is COMPUTED from where the camera actually settles, not assumed. This
 * used to hard-code a -63.435 degree pitch, which is exactly `-atan(0.2 / 0.1)`
 * - the angle that works only if the player falls exactly 0.2 units on the first
 * step. #566 (step onto ledges) legitimately changed that: the player no longer
 * falls here at all, the camera ends up level with the item rather than 0.2
 * below it, and the fixed pitch aimed the ray well past it. Deriving the angle
 * from the measured geometry means a future physics change moves the camera
 * without silently breaking every caller's aim.
 *
 * This is deliberately Earth-training-specific. Callers targeting different
 * geometry must provide and document a clear-ray staging configuration.
 */
/** Pitch, in degrees, that points the camera at `item` from where it now is.
 * Negative looks up, matching the runtime's convention. */
async function aimPitchDeg(
  game: GameServer,
  item: EntitySummary,
  staging: EarthWorldUseStaging,
): Promise<number> {
  const [ix, iy] = (await game.entities.detail(item.id)).position;
  const player = (await game.info()).player.position;
  const cameraY = player[1] + EYE_HEIGHT;
  const horizontal = Math.abs(ix - player[0]) || Math.abs(staging.horizontalOffset);
  return -(Math.atan2(iy - cameraY, horizontal) * 180) / Math.PI;
}

export async function earthWorldUse(
  game: GameServer,
  item: EntitySummary,
  staging: EarthWorldUseStaging = DEFAULT_STAGING,
): Promise<void> {
  const [x, y, z] = (await game.entities.detail(item.id)).position;
  await game.player.teleport({
    x: x + staging.horizontalOffset,
    y: y + staging.verticalOffset,
    z,
  });
  // Aim from where the camera actually is. Deliberately WITHOUT stepping first:
  // an extra frame here advances the simulation, and the hacking tests' RNG is
  // sequenced off the frame count, so a settle step silently changes their
  // outcomes.
  const pitchDeg = staging.pitchDeg ?? (await aimPitchDeg(game, item, staging));
  await game.input.set("head.look", [0, pitchDeg]);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
}
