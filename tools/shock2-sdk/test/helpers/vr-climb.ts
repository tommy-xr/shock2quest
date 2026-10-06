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

/** A real ledge-station rung/rail, rather than the old solid ladder face. */
export async function ledgeLadderHold(game: GameServer, height: number, rail = false): Promise<Vec3> {
  const ladder = (await game.entities.byTemplate(-2558)).find(e => Math.abs(e.position[2]) < 0.01);
  if (!ladder) throw new Error("debug_ladder ledge ladder missing");
  const holds = await game.physics.ladder(ladder.id);
  if (rail) {
    const [bottom, top] = holds.rails.reduce((a, b) => a[1][2] < b[1][2] ? a : b);
    const t = Math.max(0, Math.min(1, (height - bottom[1]) / (top[1] - bottom[1])));
    return bottom.map((v, i) => v + (top[i] - v) * t) as Vec3;
  }
  const [a, b] = holds.rungs.reduce((a, b) => Math.abs(a[0][1] - height) < Math.abs(b[0][1] - height) ? a : b);
  return a.map((v, i) => (v + b[i]) / 2) as Vec3;
}
