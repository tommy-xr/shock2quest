import assert from "node:assert/strict";
import { test } from "node:test";

import { defaultHoldStyle, highestHold, ladderHoldPoints } from "../src/vr-climb.js";
import type { LadderHolds, Vec3 } from "../src/types.js";

// debug_ladder's ledge-station ladder as GET /v1/physics/ladder reports it:
// face at x = -6.9, rails at z = +-0.4, rungs every 0.8.
const rickLadder: LadderHolds = {
  entity_id: 8,
  model: "ricklad6",
  normal: [1, 0, 0],
  rungs: [0.4, 1.2, 2.0].map((y) => [[-6.9, y, 0.4], [-6.9, y, -0.4]] as [Vec3, Vec3]),
  rails: [
    [[-6.9, 0, 0.4], [-6.9, 6.4, 0.4]],
    [[-6.9, 0, -0.4], [-6.9, 6.4, -0.4]],
  ],
};
// A climber in front of it (+x side) faces -x, so their right is -z.
const climber: Vec3 = [-6.3, 1.24, 0];
const right: Vec3 = [0, 0, -1];

const round = (points: Vec3[]) => points.map((p) => p.map((c) => Math.round(c * 100) / 100));

test("rung holds sit mid-way along each hand's half of the rung, off the climber's side of the face", () => {
  assert.deepEqual(round(ladderHoldPoints(rickLadder, "right", "rung", climber, right)), [
    [-6.85, 0.4, -0.2],
    [-6.85, 1.2, -0.2],
    [-6.85, 2, -0.2],
  ]);
  assert.deepEqual(round(ladderHoldPoints(rickLadder, "left", "rung", climber, right))[0], [-6.85, 0.4, 0.2]);
});

test("from behind the ladder the holds stand off the other side and the hands swap halves", () => {
  const behind = ladderHoldPoints(rickLadder, "right", "rung", [-7.5, 1.24, 0], [0, 0, 1]);
  assert.deepEqual(round(behind)[0], [-6.95, 0.4, 0.2]);
});

test("edge holds run up the rail on the hand's side", () => {
  const points = round(ladderHoldPoints(rickLadder, "right", "edge", climber, right));
  assert.equal(points.length, 65);
  assert.deepEqual(points[0], [-6.85, 0, -0.4]);
  assert.deepEqual(points[64], [-6.85, 6.4, -0.4]);
  assert.ok(points.every((p) => p[2] === -0.4));
});

test("a single-rail (pole) ladder serves both hands' edge holds", () => {
  const pole: LadderHolds = { ...rickLadder, rungs: [], rails: [rickLadder.rails[0]] };
  assert.equal(defaultHoldStyle(pole), "edge");
  assert.equal(defaultHoldStyle(rickLadder), "rung");
  const left = ladderHoldPoints(pole, "left", "edge", climber, right);
  assert.deepEqual(left, ladderHoldPoints(pole, "right", "edge", climber, right));
});

test("highestHold takes the highest hold the runtime accepts, trying top down", async () => {
  const points: Vec3[] = [0.4, 1.2, 2.0, 2.8].map((y) => [0, y, 0]);
  const tried: number[] = [];
  const reachable = async (p: Vec3) => {
    tried.push(p[1]);
    return p[1] <= 2.0;
  };
  assert.deepEqual(await highestHold(points, reachable), [0, 2.0, 0]);
  assert.deepEqual(tried, [2.8, 2.0]);
  assert.equal(await highestHold(points, async () => false), null);
});
