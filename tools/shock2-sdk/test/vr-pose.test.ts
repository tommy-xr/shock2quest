import assert from "node:assert/strict";
import { test } from "node:test";
import { handPoseAimedAt } from "../src/vr-pose.js";
import type { Quat, Vec3 } from "../src/types.js";
import { add, dot, normalize, quatRotate, sub } from "../src/vec.js";

// A pawn yawed 90 degrees, so pawn-local and world differ.
const pawn: Vec3 = [10, 0, 5];
const pawnRotation: Quat = [0, Math.SQRT1_2, 0, Math.SQRT1_2];
const eyeHeight = 1.5;

test("the posed hand's -Z ray hits the target from its offset", () => {
  const target: Vec3 = [4, 1, 5];
  const pose = handPoseAimedAt(pawn, pawnRotation, eyeHeight, target, [0.15, -0.2, -0.4]);
  const world = add(pawn, quatRotate(pawnRotation, pose.position));
  const ray = quatRotate(pawnRotation, quatRotate(pose.rotation, [0, 0, -1]));
  assert.ok(dot(ray, normalize(sub(target, world))) > 0.9999);
});

test("offsets are in the head's yaw frame toward the target", () => {
  // Target straight down -X from the eye: "forward" is -X, "right" is -Z.
  const target: Vec3 = [0, eyeHeight, 5];
  const pose = handPoseAimedAt(pawn, pawnRotation, eyeHeight, target, [0.2, 0, -0.5]);
  const world = add(pawn, quatRotate(pawnRotation, pose.position));
  const fromEye = sub(world, add(pawn, [0, eyeHeight, 0]));
  assert.ok(Math.abs(fromEye[0] + 0.5) < 1e-6, `forward ${fromEye}`);
  assert.ok(Math.abs(fromEye[2] + 0.2) < 1e-6, `right ${fromEye}`);
});
