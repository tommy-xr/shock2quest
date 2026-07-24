import type { GameServer } from "../../src/index.js";
import type { EntitySummary } from "../../src/types.js";

export interface EarthWorldUseStaging {
  horizontalOffset: number;
  verticalOffset: number;
  pitchDeg: number;
}

const DEFAULT_STAGING: EarthWorldUseStaging = {
  horizontalOffset: 0.1,
  verticalOffset: -1.6,
  pitchDeg: -63.435,
};

/**
 * Aim at and world-use an authored entity through the normal flat crosshair
 * and squeeze input. Teleporting only stages the camera; acquisition still
 * flows through the game's real interaction controller.
 *
 * The close staging is intentional for Earth training: booth geometry sits
 * between ordinary standing positions and the display items. The first fixed
 * step applies 0.2 units of gravity before interaction. Starting 1.6 below
 * the item's center therefore leaves the camera 0.2 below it; the fixed upward
 * pitch aims the ray through the center from 0.1 units away.
 *
 * This is deliberately Earth-training-specific. Callers targeting different
 * geometry must provide and document a clear-ray staging configuration.
 */
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
  await game.input.set("head.look", [0, staging.pitchDeg]);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
}
