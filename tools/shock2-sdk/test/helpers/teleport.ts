import type { GameServer } from "../../src/index.js";

/**
 * Teleport the player and verify it took effect, retrying a few times.
 * Teleports have been observed to intermittently no-op (reported success,
 * position unchanged - see issue #383); a stranded player silently
 * invalidates line-of-sight scenarios.
 */
export async function teleportVerified(
  game: GameServer,
  target: { x: number; y: number; z: number },
): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    await game.player.teleport(target);
    await game.step({ frames: 5 });
    const pos = await game.player.position();
    const dx = pos.x - target.x;
    const dz = pos.z - target.z;
    if (Math.sqrt(dx * dx + dz * dz) < 3) {
      return;
    }
  }
  throw new Error("teleport did not take effect after 3 attempts");
}
