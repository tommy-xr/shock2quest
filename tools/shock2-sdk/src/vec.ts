// Vector/quaternion math shared by VR posing (quaternions are [x, y, z, w]).
import type { Quat, Vec3 } from "./types.js";

export type { Quat };

export const add = (a: Vec3, b: Vec3): Vec3 => [
  a[0] + b[0],
  a[1] + b[1],
  a[2] + b[2],
];
export const sub = (a: Vec3, b: Vec3): Vec3 => [
  a[0] - b[0],
  a[1] - b[1],
  a[2] - b[2],
];
export const scale = (v: Vec3, amount: number): Vec3 => [
  v[0] * amount,
  v[1] * amount,
  v[2] * amount,
];
export const dot = (a: Vec3, b: Vec3): number =>
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
export const normalize = (v: Vec3): Vec3 => scale(v, 1 / Math.sqrt(dot(v, v)));

export const quatConjugate = ([x, y, z, w]: Quat): Quat => [-x, -y, -z, w];
export const quatMultiply = (
  [ax, ay, az, aw]: Quat,
  [bx, by, bz, bw]: Quat,
): Quat => [
  aw * bx + ax * bw + ay * bz - az * by,
  aw * by - ax * bz + ay * bw + az * bx,
  aw * bz + ax * by - ay * bx + az * bw,
  aw * bw - ax * bx - ay * by - az * bz,
];
export const quatNormalize = (q: Quat): Quat => {
  const length = Math.sqrt(q.reduce((sum, value) => sum + value * value, 0));
  return q.map((value) => value / length) as Quat;
};

export const quatRotate = (q: Quat, v: Vec3): Vec3 =>
  quatMultiply(quatMultiply(q, [...v, 0]), quatConjugate(q)).slice(
    0,
    3,
  ) as Vec3;

export function quatFromTo(from: Vec3, to: Vec3): Quat {
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
