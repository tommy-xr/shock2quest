import type { GameServer } from "../../src/index.js";
import type { EntitySummary } from "../../src/types.js";
import { teleportVerified } from "./teleport.js";

// Re-exported for existing importers - the balance helper itself is not
// replicator-specific, so it lives in ./nanites.js alongside stackCount.
export { carriedNaniteTotal } from "./nanites.js";

/**
 * Stage at Earth Technical Training's clear standing point and physically frob
 * the authored replicator through the normal crosshair/squeeze path.
 */
export async function physicallyOpenEarthReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  const [x, _y, z] = (await game.entities.detail(replicator.id)).position;
  await teleportVerified(game, { x: x - 1.59, y: 21.404, z: z - 2.23 });
  await game.input.set("head.look", [-35.3, -22]);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
}
