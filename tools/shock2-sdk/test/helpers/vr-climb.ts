import type { GameServer } from "../../src/index.js";
import type { Vec3 } from "../../src/types.js";

import { add, quatConjugate, quatRotate, scale, sub } from "./vr-hand.js";

/**
 * A world point as a hand channel value.
 *
 * Hand channels are in PAWN space, and a climbing pawn is exactly what moves,
 * so the conversion has to be re-derived from the pawn's pose at the moment the
 * hand reaches for the point. Once the hand is on a hold, though, it is held
 * still in pawn space (a real player's hand stays where their arm put it, in
 * the room) and the body travels underneath it - so a pull is a pawn-space
 * offset from the grab pose, NOT a fresh world target each frame.
 */
export async function vrHandLocal(
  game: GameServer,
  world: Vec3,
): Promise<Vec3> {
  const { player } = await game.info();
  return quatRotate(quatConjugate(player.rotation), sub(world, player.position));
}

/** A world-space displacement in pawn space (rotation only). */
export async function vrHandLocalDelta(
  game: GameServer,
  worldDelta: Vec3,
): Promise<Vec3> {
  const { player } = await game.info();
  return quatRotate(quatConjugate(player.rotation), worldDelta);
}

export interface VrClimbPullResult {
  before: Vec3;
  after: Vec3;
  /** The pawn position after every pull frame, in order - for continuity
   * checks, since a handoff or a break must not teleport the body. */
  path: Vec3[];
}

/**
 * Grab a hold with one VR hand and pull, one frame at a time.
 *
 * `grabAt` is the world point the hand closes on; `pull` is how far the player
 * then moves that hand, spread over `frames`. Pulling a gripped hand DOWN is
 * what lifts the body up.
 */
export async function vrClimbPull(
  game: GameServer,
  {
    hand = "right",
    grabAt,
    pull,
    frames,
    release = false,
  }: {
    hand?: "left" | "right";
    grabAt: Vec3;
    pull: Vec3;
    frames: number;
    release?: boolean;
  },
): Promise<VrClimbPullResult> {
  const before = (await game.info()).player.position;
  const atGrab = await vrHandLocal(game, grabAt);
  const pullLocal = await vrHandLocalDelta(game, pull);

  // Reach out with an OPEN hand first: the grab is a squeeze edge, so the
  // squeeze has to be down on a frame where it was up on the one before.
  await game.input.set(`${hand}_hand.position`, atGrab);
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 1 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 1 });

  const path: Vec3[] = [];
  for (let frame = 1; frame <= frames; frame += 1) {
    await game.input.set(
      `${hand}_hand.position`,
      add(atGrab, scale(pullLocal, frame / frames)),
    );
    await game.step({ frames: 1 });
    path.push((await game.info()).player.position);
  }

  if (release) {
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 1 });
  }

  return { before, after: (await game.info()).player.position, path };
}
