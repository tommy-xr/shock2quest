import type { GameServer } from "../../src/index.js";
import type { Vec3 } from "../../src/types.js";

import { quatConjugate, quatRotate, sub, type Hand, type Quat } from "./vr-hand.js";

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
  hand: Hand = "right",
): Promise<Vec3> {
  const { player, inputs } = await game.info();
  const local = quatRotate(quatConjugate(player.rotation), sub(world, player.position));
  const raw = inputs as { hands: Record<Hand, { rotation: Quat }> };
  const forward = (await game.devParams.list()).params.find(p => p.key === "glove_forward_cm")!.value;
  // A requested hold is a calibrated hand point; input channels accept the
  // raw controller pose. Undo the physical offset (0.3048 meters/foot * SCALE_FACTOR 2.5).
  const offset = quatRotate(raw.hands[hand].rotation, [0, 0, -forward * 0.01 / 0.762]);
  return sub(local, offset);
}

/** A world-space displacement in pawn space (rotation only). */
export async function vrHandLocalDelta(
  game: GameServer,
  worldDelta: Vec3,
): Promise<Vec3> {
  const { player } = await game.info();
  return quatRotate(quatConjugate(player.rotation), worldDelta);
}
