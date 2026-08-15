import type { GameServer } from "../../src/index.js";
import type { Vec3 } from "../../src/types.js";

export type Quat = [number, number, number, number];

export const add = (a: Vec3, b: Vec3): Vec3 => [
  a[0] + b[0],
  a[1] + b[1],
  a[2] + b[2],
];
const sub = (a: Vec3, b: Vec3): Vec3 => [
  a[0] - b[0],
  a[1] - b[1],
  a[2] - b[2],
];
const scale = (v: Vec3, amount: number): Vec3 => [
  v[0] * amount,
  v[1] * amount,
  v[2] * amount,
];
const dot = (a: Vec3, b: Vec3): number =>
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const normalize = (v: Vec3): Vec3 => scale(v, 1 / Math.sqrt(dot(v, v)));

const quatConjugate = ([x, y, z, w]: Quat): Quat => [-x, -y, -z, w];
const quatMultiply = (
  [ax, ay, az, aw]: Quat,
  [bx, by, bz, bw]: Quat,
): Quat => [
  aw * bx + ax * bw + ay * bz - az * by,
  aw * by - ax * bz + ay * bw + az * bx,
  aw * bz + ax * by - ay * bx + az * bw,
  aw * bw - ax * bx - ay * by - az * bz,
];
const quatNormalize = (q: Quat): Quat => {
  const length = Math.sqrt(q.reduce((sum, value) => sum + value * value, 0));
  return q.map((value) => value / length) as Quat;
};

export const quatRotate = (q: Quat, v: Vec3): Vec3 =>
  quatMultiply(quatMultiply(q, [...v, 0]), quatConjugate(q)).slice(
    0,
    3,
  ) as Vec3;

function quatFromTo(from: Vec3, to: Vec3): Quat {
  const a = normalize(from);
  const b = normalize(to);
  const d = dot(a, b);
  if (d < -0.999999) {
    const axis =
      Math.abs(a[0]) < 0.9
        ? normalize(cross(a, [1, 0, 0]))
        : normalize(cross(a, [0, 1, 0]));
    return [axis[0], axis[1], axis[2], 0];
  }
  return quatNormalize([...cross(a, b), 1 + d]);
}

/** Aim the production VR hand ray at a world point without direct entity
 * messages. Pass `squeeze = 1` to preserve an already-held item while aiming. */
export async function aimVrHandAt(
  game: GameServer,
  target: Vec3,
  standOff = 0.45,
  squeeze = 0,
): Promise<{ start: Vec3; target: Vec3 }> {
  const snapshot = await game.info();
  const pawn = snapshot.player.position;
  const pawnRotation = snapshot.player.rotation;
  const eye = add(pawn, [0, snapshot.player.camera_offset[1], 0]);
  const toward = normalize(sub(target, eye));
  const worldHand = sub(target, scale(toward, standOff));
  const worldHandRotation = quatFromTo(
    [0, 0, -1],
    normalize(sub(target, worldHand)),
  );
  const inversePawn = quatConjugate(pawnRotation);
  const localHand = quatRotate(inversePawn, sub(worldHand, pawn));
  const localHandRotation = quatNormalize(
    quatMultiply(inversePawn, worldHandRotation),
  );

  await game.input.lookAtWorldPoint(target, {
    eyeHeight: snapshot.player.camera_offset[1],
  });
  await game.input.set("right_hand.position", localHand);
  await game.input.set("right_hand.rotation", localHandRotation);
  await game.input.set("right_hand.trigger", 0);
  await game.input.set("right_hand.squeeze", squeeze);
  await game.step({ frames: 3 });
  return { start: worldHand, target };
}
