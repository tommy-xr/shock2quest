import assert from "node:assert/strict";
import { test } from "node:test";

import {
  analyzePhase,
  computeFrameMetrics,
  rootRelativeJoints,
  rotateByInverse,
  spikeThreshold,
} from "../src/anim-metrics.js";
import type { AnimationState, Quat, Vec3 } from "../src/types.js";

function makeSample(overrides: Partial<AnimationState>): AnimationState {
  return {
    entity_id: 1,
    clip: "ogsidle1",
    frame: 0,
    num_frames: 44,
    looping: false,
    remaining_time: 0,
    queue: [],
    last_clip: null,
    blend: null,
    position: [0, 0, 0],
    rotation: [0, 0, 0, 1],
    joints: [[0, 0, 0]],
    ...overrides,
  };
}

test("rotateByInverse undoes a 90-degree yaw", () => {
  const halfSqrt2 = Math.SQRT1_2;
  // 90 degrees about +Y: rotates +X to -Z. The inverse maps -Z back to +X.
  const q: Quat = [0, halfSqrt2, 0, halfSqrt2];
  const result = rotateByInverse([0, 0, -1], q);
  assert.ok(Math.abs(result[0] - 1) < 1e-6, `x: ${result[0]}`);
  assert.ok(Math.abs(result[1]) < 1e-6, `y: ${result[1]}`);
  assert.ok(Math.abs(result[2]) < 1e-6, `z: ${result[2]}`);
});

test("rootRelativeJoints removes translation and rotation", () => {
  const halfSqrt2 = Math.SQRT1_2;
  const localOffset: Vec3 = [1, 2, 0];
  // Entity at [10, 0, 5], yawed 90 degrees: the local +X offset lands at -Z in
  // world space.
  const sample = makeSample({
    position: [10, 0, 5],
    rotation: [0, halfSqrt2, 0, halfSqrt2],
    joints: [[10, 2, 5 - 1]],
  });
  const [joint] = rootRelativeJoints(sample);
  for (let axis = 0; axis < 3; axis++) {
    assert.ok(
      Math.abs(joint[axis] - localOffset[axis]) < 1e-5,
      `axis ${axis}: ${joint[axis]} != ${localOffset[axis]}`,
    );
  }
});

test("root-relative metrics isolate pose pops from locomotion", () => {
  // Entity glides +X at 0.1/frame with a rigid pose: world deltas are pure
  // locomotion, local deltas are ~0.
  const samples = Array.from({ length: 10 }, (_, t) =>
    makeSample({
      frame: t,
      position: [t * 0.1, 0, 0],
      joints: [
        [t * 0.1, 1, 0],
        [t * 0.1 + 0.5, 1.5, 0],
      ],
    }),
  );
  const metrics = computeFrameMetrics(samples);
  for (const m of metrics) {
    assert.ok(Math.abs(m.worldMaxDelta - 0.1) < 1e-6, `world ${m.worldMaxDelta}`);
    assert.ok(m.localMaxDelta < 1e-6, `local ${m.localMaxDelta}`);
    assert.ok(Math.abs(m.entityDelta - 0.1) < 1e-6);
    assert.ok(m.entityAccel < 1e-6);
  }
});

test("spikes at a clip change are classified clip-switch", () => {
  const samples: AnimationState[] = [];
  for (let t = 0; t < 40; t++) {
    const changed = t >= 20;
    samples.push(
      makeSample({
        clip: changed ? "ogsrunl" : "ogsidle1",
        frame: changed ? t - 20 : t,
        // Small continuous drift, then a 1-unit pop exactly at the switch.
        joints: [[t * 0.01 + (changed ? 1 : 0), 1, 0]],
      }),
    );
  }
  const report = analyzePhase(samples, { minSpike: 0.1 });
  assert.equal(report.clipChangeCount, 1);
  assert.equal(report.spikes.length, 1);
  assert.equal(report.spikes[0].kind, "clip-switch");
  assert.equal(report.spikes[0].index, 20);
  assert.ok(Math.abs(report.spikes[0].magnitude - 1) < 0.02);
  assert.equal(report.spikes[0].prevClip, "ogsidle1");
  assert.equal(report.spikes[0].clip, "ogsrunl");
});

test("spikes at a frame reset of the same clip are classified loop-seam", () => {
  const samples: AnimationState[] = [];
  for (let t = 0; t < 40; t++) {
    const frame = t % 20; // clip restarts at t=20 without a name change
    samples.push(
      makeSample({
        clip: "ogsrunl",
        frame,
        joints: [[frame * 0.01 + (t === 20 ? 0.8 : 0), 1, 0]],
      }),
    );
  }
  const report = analyzePhase(samples, { minSpike: 0.1 });
  assert.equal(report.loopResetCount, 1);
  // The pop enters at the reset and leaves the next frame; both deltas are
  // attributed to the reset (a pose update can trail the queue bookkeeping
  // by one sample).
  assert.equal(report.spikes.length, 2);
  for (const spike of report.spikes) {
    assert.equal(spike.kind, "loop-seam");
  }
});

test("mid-clip spikes are classified mid-clip", () => {
  const samples = Array.from({ length: 40 }, (_, t) =>
    makeSample({
      frame: t,
      num_frames: 100,
      joints: [[t * 0.01 + (t === 25 ? 0.7 : 0), 1, 0]],
    }),
  );
  const report = analyzePhase(samples, { minSpike: 0.1 });
  assert.equal(report.spikes.length, 2, "the pop enters and leaves (two deltas)");
  for (const spike of report.spikes) {
    assert.equal(spike.kind, "mid-clip");
  }
});

test("spikeThreshold is robust to the spikes themselves and floored", () => {
  // Alternating 0 / 0.1 stride (30fps clips sampled at 60Hz) with 1.0 pops.
  const values: number[] = [];
  for (let t = 0; t < 100; t++) values.push(t % 2 === 0 ? 0 : 0.1);
  values[15] = 1.0;
  values[45] = 1.0;
  const threshold = spikeThreshold(values, 5, 0.1);
  assert.ok(threshold < 1.0, `threshold ${threshold} should not swallow the pops`);
  assert.ok(threshold >= 0.1, "floored at minSpike");

  // All-zero series: MAD and std collapse, floor wins.
  assert.equal(spikeThreshold(new Array(50).fill(0), 5, 0.1), 0.1);
});

test("analyzePhase summary stats and rates", () => {
  const samples = Array.from({ length: 61 }, (_, t) =>
    makeSample({ frame: t, num_frames: 100, joints: [[t * 0.05, 1, 0]] }),
  );
  const report = analyzePhase(samples, { fps: 60 });
  assert.equal(report.sampleCount, 61);
  assert.ok(Math.abs(report.durationSeconds - 1) < 1e-9);
  assert.equal(report.spikes.length, 0);
  assert.ok(Math.abs(report.localMaxDelta.p50 - 0.05) < 1e-6);
  assert.ok(Math.abs(report.localMaxDelta.max - 0.05) < 1e-6);
  assert.equal(report.clipFrames["ogsidle1"], 61);
});
