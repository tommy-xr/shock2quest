// A clip's recorded controller inputs, `<clip>.inputs.json` beside its mp4.
// Written by the SDK's recordClipInputs; the format is documented in
// tools/shock2-sdk/src/clip-inputs.ts.

// Value held at t: the last keyframe at or before it, else `rest`.
function held(frames, t, rest) {
  let value = rest;
  for (const [at, v] of frames ?? []) {
    if (at > t) break;
    value = v;
  }
  return value;
}

// Every recorded part's state at `t` seconds, per hand, e.g.
// { L: { grip: { pose: [1], pressed: true } }, R: { stick: { pose: [0.5, 0, 0], pressed: true } } }.
// `pose` holds 0..1 fractions along the part's glTF visual responses (min -> max):
// [x, y, click] for a stick, [value] otherwise.
export function sampleInputs(timeline, t) {
  const state = { L: {}, R: {} };
  for (const key of Object.keys(timeline.parts)) {
    const [hand, part] = key.split(".");
    if (part === "stick" || part === "click") {
      const [x, y] = held(timeline.parts[`${hand}.stick`], t, [0, 0]);
      const click = held(timeline.parts[`${hand}.click`], t, 0);
      // WebXR thumbstick axes run -1..1 with up = -1; recorded +y is forward.
      state[hand].stick = { pose: [(x + 1) / 2, (1 - y) / 2, click], pressed: Math.hypot(x, y) > 0.15 || click > 0.1 };
    } else {
      const value = held(timeline.parts[key], t, 0);
      state[hand][part] = { pose: [value], pressed: value > 0.1 };
    }
  }
  return state;
}

// The timeline at `url`, or null when the clip has none.
export async function fetchInputs(url) {
  try {
    const r = await fetch(url);
    return r.ok ? await r.json() : null;
  } catch {
    return null;
  }
}
