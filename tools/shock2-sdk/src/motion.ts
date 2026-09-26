// Keyframed, eased motion with a little seeded sway, so scripted VR hands and
// head move like a person rather than a robot. Pure: sample at time t.
import type { Vec3 } from "./types.js";

export interface Keyframe<T> {
  /** Seconds from the clip's start. */
  t: number;
  value: T;
}

/** Ease-in-out: zero velocity at each keyframe. */
const ease = (x: number) => x * x * (3 - 2 * x);

/** Sample a track at `t`, easing between neighbouring keyframes. */
export function sampleTrack(keys: Keyframe<number>[], t: number): number;
export function sampleTrack(keys: Keyframe<Vec3>[], t: number): Vec3;
export function sampleTrack(keys: Keyframe<number | Vec3>[], t: number): number | Vec3 {
  if (keys.length === 0) throw new Error("a track needs at least one keyframe");
  const next = keys.findIndex((k) => k.t > t);
  if (next === 0) return keys[0].value;
  if (next === -1) return keys[keys.length - 1].value;
  const a = keys[next - 1];
  const b = keys[next];
  const u = ease((t - a.t) / (b.t - a.t));
  const mix = (x: number, y: number) => x + (y - x) * u;
  return typeof a.value === "number"
    ? mix(a.value, b.value as number)
    : (a.value.map((x, i) => mix(x, (b.value as Vec3)[i])) as Vec3);
}

/**
 * Deterministic low-frequency drift, per axis a sum of two incommensurate
 * sines with seeded phases: breathing/hand-tremor scale motion that never
 * visibly repeats within a clip.
 */
export function sway(seed: number, t: number, amplitude: number, hz = 0.35): Vec3 {
  const phase = (k: number) => {
    const x = Math.sin(seed * 12.9898 + k * 78.233) * 43758.5453;
    return (x - Math.floor(x)) * Math.PI * 2;
  };
  const axis = (k: number) =>
    amplitude *
    (0.6 * Math.sin(2 * Math.PI * hz * t + phase(k)) +
      0.4 * Math.sin(2 * Math.PI * hz * 2.37 * t + phase(k + 3)));
  return [axis(0), axis(1), axis(2)];
}
