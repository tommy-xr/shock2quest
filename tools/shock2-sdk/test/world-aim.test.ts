import assert from "node:assert/strict";
import test from "node:test";
import { headRotationForWorldPoint } from "../src/index.js";

type Quat = [number, number, number, number];

function multiplyQuat(a: Quat, b: Quat): Quat {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

test("world aim cancels a non-identity save-restored pawn rotation", () => {
  const pawn: Quat = [0, Math.SQRT1_2, 0, Math.SQRT1_2];
  const head = headRotationForWorldPoint([5, 2, 5], pawn, [-5, 3.6, 5]);
  const composed = multiplyQuat(pawn, head);
  const expected = headRotationForWorldPoint(
    [5, 2, 5],
    [0, 0, 0, 1],
    [-5, 3.6, 5],
  );
  composed.forEach((component, index) => {
    assert.ok(Math.abs(component - expected[index]) < 1e-6);
  });
  assert.notDeepEqual(head, expected, "raw world rotation would aim incorrectly");
});

test("world aim rejects an undefined zero-length direction", () => {
  assert.throws(
    () => headRotationForWorldPoint([1, 2, 3], [0, 0, 0, 1], [1, 3.6, 3]),
    /target must differ/,
  );
});
