import assert from "node:assert/strict";
import { test } from "node:test";
import { sampleTrack, sway } from "../src/motion.js";
import type { Vec3 } from "../src/types.js";

test("tracks hold their ends and ease through the middle", () => {
  const keys = [{ t: 1, value: 0 }, { t: 3, value: 10 }];
  assert.equal(sampleTrack(keys, 0), 0);
  assert.equal(sampleTrack(keys, 5), 10);
  assert.equal(sampleTrack(keys, 2), 5);
  // Eased: slower than linear near a keyframe.
  assert.ok(sampleTrack(keys, 1.2) < 1);
});

test("out-of-order keyframes are rejected", () => {
  const keys = [{ t: 1, value: 10 }, { t: 0, value: 0 }];
  assert.throws(() => sampleTrack(keys, 0.5), /strictly increase/);
});

test("vector tracks interpolate per component", () => {
  const keys: { t: number; value: Vec3 }[] = [
    { t: 0, value: [0, 0, 0] },
    { t: 1, value: [2, -4, 6] },
  ];
  assert.deepEqual(sampleTrack(keys, 0.5), [1, -2, 3]);
});

test("sway is deterministic, bounded and seed-dependent", () => {
  assert.deepEqual(sway(1, 0.7, 0.01), sway(1, 0.7, 0.01));
  assert.notDeepEqual(sway(1, 0.7, 0.01), sway(2, 0.7, 0.01));
  for (let t = 0; t < 10; t += 0.1) {
    for (const v of sway(3, t, 0.01)) assert.ok(Math.abs(v) <= 0.01 + 1e-12);
  }
});
