// Stage VR hands for captures: place each controller at an offset from the
// eye (in the head's yaw frame) and aim it at a world point.
import { type Game, lookQuat } from "./game.js";
import type { Quat, Vec3 } from "./types.js";
import {
  add,
  quatConjugate,
  quatMultiply,
  quatNormalize,
  quatRotate,
  sub,
} from "./vec.js";

export type Hand = "left" | "right";

/**
 * Pawn-local controller pose (the space `<hand>_hand.position/rotation` take)
 * for a hand at `offset` from the eye - x right, y up, -z toward `target`,
 * horizontally - whose -Z ray points at `aim` (default `target`).
 */
export function handPoseAimedAt(
  pawnPosition: Vec3,
  pawnRotation: Quat,
  eyeHeight: number,
  target: Vec3,
  offset: Vec3,
  aim: Vec3 = target,
): { position: Vec3; rotation: Quat } {
  const eye = add(pawnPosition, [0, eyeHeight, 0]);
  const toward = sub(target, eye);
  const yaw = Math.atan2(-toward[0], -toward[2]);
  const yawRotation: Quat = [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
  const world = add(eye, quatRotate(yawRotation, offset));
  // lookQuat, not a shortest arc from -Z: that picks an arbitrary roll (a
  // gun aimed along +Z comes out upside down).
  const worldRotation = lookQuat(sub(aim, world));
  const inversePawn = quatConjugate(pawnRotation);
  return {
    position: quatRotate(inversePawn, sub(world, pawnPosition)),
    rotation: quatNormalize(quatMultiply(inversePawn, worldRotation)),
  };
}

/** Signed yaw (radians) from the pawn's facing to `target`, positive = left. */
export function pawnYawError(pawnPosition: Vec3, pawnRotation: Quat, target: Vec3): number {
  const facing = quatRotate(pawnRotation, [0, 0, -1]);
  const toward = sub(target, pawnPosition);
  const error = Math.atan2(-toward[0], -toward[2]) - Math.atan2(-facing[0], -facing[2]);
  return Math.atan2(Math.sin(error), Math.cos(error));
}

/**
 * Turn the pawn to face `target` with the turn stick, the way a player does,
 * so the body (and its shoulder/holster zones) faces it rather than only the
 * head. Steps frames until within `toleranceDeg`.
 */
export async function turnPawnToward(game: Game, target: Vec3, toleranceDeg = 5): Promise<void> {
  for (let frame = 0; frame < 600; frame++) {
    const { player } = await game.info();
    const error = pawnYawError(player.position, player.rotation, target);
    if (Math.abs(error) <= (toleranceDeg * Math.PI) / 180) {
      await game.input.set("left_hand.thumbstick", [0, 0]);
      return;
    }
    // Stick x > 0 turns left (positive yaw); ease off near the target.
    const push = Math.min(1, Math.max(0.2, Math.abs(error)));
    await game.input.set("left_hand.thumbstick", [error > 0 ? push : -push, 0]);
    await game.step({ frames: 1 });
  }
  await game.input.set("left_hand.thumbstick", [0, 0]);
  throw new Error("pawn did not turn toward the target within 600 frames");
}

/**
 * Square the player up to `target`: park both hands low in front, turn the
 * pawn toward it, look at it, and let the tracked body yaw settle. Body zones
 * (shoulder backpack, holsters) freeze their yaw while a hand is inside one,
 * and the default hand pose starts inside, so posing without this leaves the
 * zones facing the old direction - a later squeeze then "draws" from a
 * shoulder instead of holding.
 */
export async function faceTarget(game: Game, target: Vec3): Promise<void> {
  for (const hand of ["left", "right"] as const) {
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -0.2 : 0.2, 0.3, -0.6]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  }
  await turnPawnToward(game, target);
  await game.input.lookAtWorldPoint(target);
  // Let the zones' eased (0.5 s time constant) yaw catch up with the head.
  await game.step({ frames: 60 });
}

/**
 * Turn the head to `target`, then aim each given hand from its offset at
 * `aim` (default `target`) - e.g. eyes on a head, guns on the torso.
 */
export async function aimHandsAt(
  game: Game,
  target: Vec3,
  hands: Partial<Record<Hand, Vec3>>,
  aim: Vec3 = target,
): Promise<void> {
  await game.input.lookAtWorldPoint(target);
  const { player } = await game.info();
  for (const [hand, offset] of Object.entries(hands) as [Hand, Vec3][]) {
    const pose = handPoseAimedAt(
      player.position,
      player.rotation,
      player.camera_offset[1],
      target,
      offset,
      aim,
    );
    await game.input.set(`${hand}_hand.position`, pose.position);
    await game.input.set(`${hand}_hand.rotation`, pose.rotation);
  }
}

/**
 * Put the `support` hand on the two-handed grip of the gun `primary` holds,
 * and squeeze to attach it. Steps a few frames.
 */
export async function attachSupportHand(game: Game, primary: Hand): Promise<void> {
  const support: Hand = primary === "left" ? "right" : "left";
  await game.step({ frames: 2 });
  const { player } = await game.info();
  const socket = player.hand_grips.find((g) => g.hand === primary)?.support;
  if (!socket) throw new Error(`the ${primary} hand holds nothing with a support grip`);
  const p = socket.controller_position;
  const q = socket.controller_rotation;
  const inversePawn = quatConjugate(player.rotation);
  await game.input.set(
    `${support}_hand.position`,
    quatRotate(inversePawn, sub([p.x, p.y, p.z], player.position)),
  );
  await game.input.set(
    `${support}_hand.rotation`,
    quatMultiply(inversePawn, [q.v.x, q.v.y, q.v.z, q.s]),
  );
  await game.input.set(`${support}_hand.squeeze`, 1);
  await game.step({ frames: 10 });
  const attached = (await game.info()).player.hand_grips.find((g) => g.hand === primary)
    ?.support?.attached;
  if (!attached) throw new Error(`the ${support} hand did not attach to the support grip`);
}
