/**
 * Animation smoothness metrics over a sequence of per-frame AnimationState
 * samples (GET /v1/entities/:id/animation, one sample per stepped frame).
 *
 * Two spaces are analyzed:
 * - world: raw world-space joint positions (includes locomotion), and
 * - local: root-relative joints (joint minus entity position, rotated by the
 *   inverse entity rotation) - isolates pose pops from locomotion.
 *
 * Per-frame metrics are first/second/third differences of joint positions
 * (displacement / accel / jerk), plus the entity-position second difference
 * (locomotion smoothness). Frames whose root-relative max-joint displacement
 * exceeds a threshold are flagged as spikes and attributed to the clip
 * timeline: `clip-switch` (clip name changed), `loop-seam` (same clip
 * restarted), or `mid-clip`.
 */

import type { AnimationState, Quat, Vec3 } from "./types.js";

function sub(a: Vec3, b: Vec3): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

function magnitude(a: Vec3): number {
  return Math.hypot(a[0], a[1], a[2]);
}

/** Rotate `v` by the inverse (conjugate) of the unit quaternion `q` = [x,y,z,w]. */
export function rotateByInverse(v: Vec3, q: Quat): Vec3 {
  // Conjugate of q, then v' = v + 2w(u x v) + 2(u x (u x v)) with u = q.xyz.
  const ux = -q[0];
  const uy = -q[1];
  const uz = -q[2];
  const w = q[3];
  const cx = uy * v[2] - uz * v[1];
  const cy = uz * v[0] - ux * v[2];
  const cz = ux * v[1] - uy * v[0];
  const ccx = uy * cz - uz * cy;
  const ccy = uz * cx - ux * cz;
  const ccz = ux * cy - uy * cx;
  return [
    v[0] + 2 * (w * cx + ccx),
    v[1] + 2 * (w * cy + ccy),
    v[2] + 2 * (w * cz + ccz),
  ];
}

/** Joints expressed in the entity's local (root-relative) frame. */
export function rootRelativeJoints(sample: AnimationState): Vec3[] {
  return sample.joints.map((joint) =>
    rotateByInverse(sub(joint, sample.position), sample.rotation),
  );
}

/** Differences between sample `index` and the preceding sample(s). */
export interface FrameMetric {
  /** Sample index the metric belongs to (delta from sample index-1). */
  index: number;
  clip: string | null;
  clipFrame: number;
  /** Clip name differs from the previous sample. */
  clipChanged: boolean;
  /** Same clip, but the frame index moved backwards (loop restart). */
  loopReset: boolean;
  /** A crossfade blend was active at this sample. */
  blending: boolean;
  worldMaxDelta: number;
  worldMeanDelta: number;
  localMaxDelta: number;
  localMeanDelta: number;
  /** Max per-joint second difference |p[t] - 2p[t-1] + p[t-2]| (0 until index 2). */
  worldMaxAccel: number;
  localMaxAccel: number;
  /** Max per-joint third difference (0 until index 3). */
  worldMaxJerk: number;
  localMaxJerk: number;
  /** Entity-position first difference (locomotion speed per frame). */
  entityDelta: number;
  /** Entity-position second difference (locomotion smoothness). */
  entityAccel: number;
}

export type SpikeKind = "clip-switch" | "loop-seam" | "mid-clip";

export interface SpikeInfo {
  index: number;
  kind: SpikeKind;
  /** Root-relative max-joint displacement at this frame. */
  magnitude: number;
  worldMagnitude: number;
  clip: string | null;
  prevClip: string | null;
  clipFrame: number;
  blending: boolean;
}

export interface SeriesStats {
  mean: number;
  p50: number;
  p95: number;
  max: number;
}

export interface AnalyzeOptions {
  /** Sigma multiplier for the spike threshold (default 5). */
  sigma?: number;
  /** Absolute floor for the spike threshold, world units (default 0.1). */
  minSpike?: number;
  /** Simulation rate the samples were captured at (default 60). */
  fps?: number;
}

export interface PhaseReport {
  sampleCount: number;
  durationSeconds: number;
  /** Threshold applied to the root-relative max-joint-displacement series. */
  spikeThreshold: number;
  spikes: SpikeInfo[];
  spikesPerSecond: number;
  maxSpikeMagnitude: number;
  spikeKinds: Record<SpikeKind, number>;
  clipChangeCount: number;
  loopResetCount: number;
  /** Frames spent per clip name across the capture. */
  clipFrames: Record<string, number>;
  localMaxDelta: SeriesStats;
  worldMaxDelta: SeriesStats;
  /** Mean over frames of the max per-joint jerk magnitude. */
  localMeanMaxJerk: number;
  worldMeanMaxJerk: number;
  entityDelta: SeriesStats;
  entityAccel: SeriesStats;
  frames: FrameMetric[];
}

export function computeFrameMetrics(samples: AnimationState[]): FrameMetric[] {
  const world = samples.map((s) => s.joints);
  const local = samples.map(rootRelativeJoints);
  const metrics: FrameMetric[] = [];

  for (let t = 1; t < samples.length; t++) {
    const jointCount = Math.min(world[t].length, world[t - 1].length);
    let worldMaxDelta = 0;
    let worldSumDelta = 0;
    let localMaxDelta = 0;
    let localSumDelta = 0;
    let worldMaxAccel = 0;
    let localMaxAccel = 0;
    let worldMaxJerk = 0;
    let localMaxJerk = 0;

    for (let j = 0; j < jointCount; j++) {
      const wd = magnitude(sub(world[t][j], world[t - 1][j]));
      const ld = magnitude(sub(local[t][j], local[t - 1][j]));
      worldMaxDelta = Math.max(worldMaxDelta, wd);
      localMaxDelta = Math.max(localMaxDelta, ld);
      worldSumDelta += wd;
      localSumDelta += ld;

      if (t >= 2 && j < world[t - 2].length) {
        worldMaxAccel = Math.max(
          worldMaxAccel,
          magnitude(secondDiff(world, t, j)),
        );
        localMaxAccel = Math.max(
          localMaxAccel,
          magnitude(secondDiff(local, t, j)),
        );
        if (t >= 3 && j < world[t - 3].length) {
          worldMaxJerk = Math.max(
            worldMaxJerk,
            magnitude(thirdDiff(world, t, j)),
          );
          localMaxJerk = Math.max(
            localMaxJerk,
            magnitude(thirdDiff(local, t, j)),
          );
        }
      }
    }

    const entityDelta = magnitude(
      sub(samples[t].position, samples[t - 1].position),
    );
    const entityAccel =
      t >= 2
        ? magnitude([
            samples[t].position[0] -
              2 * samples[t - 1].position[0] +
              samples[t - 2].position[0],
            samples[t].position[1] -
              2 * samples[t - 1].position[1] +
              samples[t - 2].position[1],
            samples[t].position[2] -
              2 * samples[t - 1].position[2] +
              samples[t - 2].position[2],
          ])
        : 0;

    const clipChanged = samples[t].clip !== samples[t - 1].clip;
    const loopReset = !clipChanged && samples[t].frame < samples[t - 1].frame;

    metrics.push({
      index: t,
      clip: samples[t].clip,
      clipFrame: samples[t].frame,
      clipChanged,
      loopReset,
      blending: samples[t].blend !== null,
      worldMaxDelta,
      worldMeanDelta: jointCount > 0 ? worldSumDelta / jointCount : 0,
      localMaxDelta,
      localMeanDelta: jointCount > 0 ? localSumDelta / jointCount : 0,
      worldMaxAccel,
      localMaxAccel,
      worldMaxJerk,
      localMaxJerk,
      entityDelta,
      entityAccel,
    });
  }

  return metrics;
}

function secondDiff(series: Vec3[][], t: number, j: number): Vec3 {
  return [
    series[t][j][0] - 2 * series[t - 1][j][0] + series[t - 2][j][0],
    series[t][j][1] - 2 * series[t - 1][j][1] + series[t - 2][j][1],
    series[t][j][2] - 2 * series[t - 1][j][2] + series[t - 2][j][2],
  ];
}

function thirdDiff(series: Vec3[][], t: number, j: number): Vec3 {
  return [
    series[t][j][0] -
      3 * series[t - 1][j][0] +
      3 * series[t - 2][j][0] -
      series[t - 3][j][0],
    series[t][j][1] -
      3 * series[t - 1][j][1] +
      3 * series[t - 2][j][1] -
      series[t - 3][j][1],
    series[t][j][2] -
      3 * series[t - 1][j][2] +
      3 * series[t - 2][j][2] -
      series[t - 3][j][2],
  ];
}

function seriesStats(values: number[]): SeriesStats {
  if (values.length === 0) {
    return { mean: 0, p50: 0, p95: 0, max: 0 };
  }
  const sorted = [...values].sort((a, b) => a - b);
  const mean = values.reduce((acc, v) => acc + v, 0) / values.length;
  return {
    mean,
    p50: percentile(sorted, 0.5),
    p95: percentile(sorted, 0.95),
    max: sorted[sorted.length - 1],
  };
}

/** `sorted` must be ascending. */
function percentile(sorted: number[], p: number): number {
  const pos = (sorted.length - 1) * p;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

/**
 * Spike threshold over the root-relative max-joint-displacement series:
 * mean + sigma * std computed over the values at or below p95 (so the seam
 * pops being hunted cannot inflate the threshold past themselves), floored
 * at `minSpike`.
 */
export function spikeThreshold(
  values: number[],
  sigma: number,
  minSpike: number,
): number {
  if (values.length === 0) return minSpike;
  const sorted = [...values].sort((a, b) => a - b);
  const p95 = percentile(sorted, 0.95);
  const trimmed = values.filter((v) => v <= p95);
  const mean = trimmed.reduce((acc, v) => acc + v, 0) / trimmed.length;
  const std = Math.sqrt(
    trimmed.reduce((acc, v) => acc + (v - mean) * (v - mean), 0) /
      trimmed.length,
  );
  return Math.max(minSpike, mean + sigma * std);
}

/**
 * Classify a spike at metric index `i` by the clip timeline. The pose can
 * update one sample after the queue-head bookkeeping changes (60 Hz sampling
 * of 30 fps clips), so the previous metric's transition also counts.
 */
function classifySpike(metrics: FrameMetric[], i: number): SpikeKind {
  const here = metrics[i];
  const prev = i > 0 ? metrics[i - 1] : undefined;
  if (here.clipChanged || prev?.clipChanged) return "clip-switch";
  if (here.loopReset || prev?.loopReset) return "loop-seam";
  return "mid-clip";
}

export function analyzePhase(
  samples: AnimationState[],
  options?: AnalyzeOptions,
): PhaseReport {
  const sigma = options?.sigma ?? 5;
  const minSpike = options?.minSpike ?? 0.1;
  const fps = options?.fps ?? 60;

  const metrics = computeFrameMetrics(samples);
  const localSeries = metrics.map((m) => m.localMaxDelta);
  const threshold = spikeThreshold(localSeries, sigma, minSpike);

  const spikes: SpikeInfo[] = [];
  for (let i = 0; i < metrics.length; i++) {
    const m = metrics[i];
    if (m.localMaxDelta <= threshold) continue;
    spikes.push({
      index: m.index,
      kind: classifySpike(metrics, i),
      magnitude: m.localMaxDelta,
      worldMagnitude: m.worldMaxDelta,
      clip: m.clip,
      prevClip: samples[m.index - 1].clip,
      clipFrame: m.clipFrame,
      blending: m.blending,
    });
  }

  const spikeKinds: Record<SpikeKind, number> = {
    "clip-switch": 0,
    "loop-seam": 0,
    "mid-clip": 0,
  };
  for (const spike of spikes) spikeKinds[spike.kind]++;

  const clipFrames: Record<string, number> = {};
  for (const sample of samples) {
    const name = sample.clip ?? "(none)";
    clipFrames[name] = (clipFrames[name] ?? 0) + 1;
  }

  const durationSeconds = Math.max(samples.length - 1, 0) / fps;
  const jerkFrames = metrics.filter((m) => m.index >= 3);

  return {
    sampleCount: samples.length,
    durationSeconds,
    spikeThreshold: threshold,
    spikes,
    spikesPerSecond: durationSeconds > 0 ? spikes.length / durationSeconds : 0,
    maxSpikeMagnitude: spikes.reduce((acc, s) => Math.max(acc, s.magnitude), 0),
    spikeKinds,
    clipChangeCount: metrics.filter((m) => m.clipChanged).length,
    loopResetCount: metrics.filter((m) => m.loopReset).length,
    clipFrames,
    localMaxDelta: seriesStats(localSeries),
    worldMaxDelta: seriesStats(metrics.map((m) => m.worldMaxDelta)),
    localMeanMaxJerk:
      jerkFrames.length > 0
        ? jerkFrames.reduce((acc, m) => acc + m.localMaxJerk, 0) /
          jerkFrames.length
        : 0,
    worldMeanMaxJerk:
      jerkFrames.length > 0
        ? jerkFrames.reduce((acc, m) => acc + m.worldMaxJerk, 0) /
          jerkFrames.length
        : 0,
    entityDelta: seriesStats(metrics.map((m) => m.entityDelta)),
    entityAccel: seriesStats(
      metrics.filter((m) => m.index >= 2).map((m) => m.entityAccel),
    ),
    frames: metrics,
  };
}
