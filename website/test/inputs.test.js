// node --test website/test/*.test.js
import assert from "node:assert/strict";
import { test } from "node:test";

import { sampleInputs } from "../shared/inputs.js";

const timeline = {
  format: "shock2quest-clip-inputs/1",
  duration: 2,
  parts: {
    "R.stick": [[0.5, [0, 1]], [1, [1, 0]]],
    "L.lower": [[0.25, 1], [0.45, 0]],
    "L.click": [[1.5, 1], [1.7, 0]],
  },
};

test("parts rest before their first keyframe", () => {
  const s = sampleInputs(timeline, 0);
  assert.deepEqual(s.R.stick, { pose: [0.5, 0.5, 0], pressed: false });
  assert.deepEqual(s.L.lower, { pose: [0], pressed: false });
});

test("a keyframe holds until the next", () => {
  assert.deepEqual(sampleInputs(timeline, 0.3).L.lower, { pose: [1], pressed: true });
  assert.deepEqual(sampleInputs(timeline, 0.45).L.lower, { pose: [0], pressed: false });
});

test("stick forward poses the y axis toward its min (WebXR up = -1)", () => {
  assert.deepEqual(sampleInputs(timeline, 0.75).R.stick, { pose: [0.5, 0, 0], pressed: true });
  assert.deepEqual(sampleInputs(timeline, 1.2).R.stick, { pose: [1, 0.5, 0], pressed: true });
});

test("a stick click folds into its stick", () => {
  assert.deepEqual(sampleInputs(timeline, 1.6).L.stick, { pose: [0.5, 0.5, 1], pressed: true });
  assert.equal(sampleInputs(timeline, 1.6).L.click, undefined);
});
