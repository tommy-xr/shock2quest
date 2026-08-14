---
name: oculus-profiling
description: >-
  Measure shock2quest startup, frame/update/render timings, compositor
  performance, memory, and Adreno GPU counters on an adb-attached Meta Quest.
  Use to baseline missions, diagnose missed VR frame budgets, compare OpenXR or
  rendering changes, evaluate FFR or multiview, and require measured device
  evidence for optimization claims.
---

# Oculus profiling

Measure before optimizing. Use the `vr-device-loop` skill first to build,
install, focus, and visually verify a release APK.

Desktop and device measurements answer different questions:

| Surface | Evidence |
| --- | --- |
| Desktop tests/benchmarks | correctness and host CPU behavior |
| `SHOCK2QUEST_PERF` | game update and render-stage CPU wall time on Quest |
| VrApi telemetry | delivered FPS, App/compositor time, stale frames, CPU/GPU load |
| `ovrgpuprofiler` | Adreno stage split, stalls, cache behavior, work volume |
| Device PNG/MP4 | the measured workload actually rendered correctly |

Never use a debug APK for performance claims. Confirm with:

```sh
adb shell dumpsys package com.tommybuilds.shock2quest | grep 'flags='
```

## Run mission baselines

Test the parser first, then measure one mission or all installed missions:

```sh
node --test \
  .claude/skills/oculus-profiling/scripts/quest-benchmark.test.mjs

node .claude/skills/oculus-profiling/scripts/quest-benchmark.mjs \
  --mission medsci1.mis --warmup 10 --seconds 30

node .claude/skills/oculus-profiling/scripts/quest-benchmark.mjs \
  --all --warmup 5 --seconds 10 --output /tmp/quest-all-levels
```

The sweep restarts the APK per mission, waits for the first submitted XR frame,
waits for focus, warms up, samples telemetry, records memory, then extracts a
focused device PNG for each successful run. It uses a direct compositor
screenshot first and falls back to a frame from a short Quest recording. The
sweep continues after a mission or visual failure and writes:

- `results.json` for machine comparison;
- `report.md` for review;
- one JSON, runtime log, raw telemetry log, and optional PNG per mission.

The startup fields deliberately separate `Game::init` from host-observed
launch-to-focused. The latter includes Android activity startup, OpenXR/EGL
setup, mission initialization, session focus, swapchain allocation, and the
first render.

The Oculus runtime requests 90 Hz when the active OpenXR runtime advertises it.
Horizon OS can transiently return an empty rate list immediately after session
begin, so the runtime probes 90 Hz in that case and accepts only the extension's
specific unsupported-rate error as a graceful fallback. `SHOCK2QUEST_READY`
records the target, requested, and active rates; use the active rate as the
frame-budget denominator and do not compare runs at different rates. The
benchmark also records later refresh-rate transitions and rejects a sample that
diverges from the requested rate.

Use identical release builds, headset refresh rate, device power state, mission
selector, spawn point, warmup, and interval for comparisons. Repeat noisy
results; do not compare a cold first run against a warmed asset cache without
labeling it.

## Read engine telemetry

`SHOCK2QUEST_PERF` aggregates one-second windows:

- `fps` / `frame_ms`: paced rendered-frame rate and interval;
- `skipped`: frames for which OpenXR said not to render;
- `update_ms`: `Game::update`;
- `scene_ms`: shared `Game::render` scene preparation;
- `left_eye_ms`, `right_eye_ms`: per-eye swapchain acquire/wait, scene
  generation plus GLES rendering, and swapchain release;
- `finish_ms`: post-eye `Game::finish_render`, currently visibility preparation;
- `submit_ms`: OpenXR `FrameStream::end`.

The two eye timings are currently sequential and exclude `finish_ms`. Their sum
also includes swapchain synchronization, so it is not pure render or GPU time.
Use VrApi and hardware counters before attributing it, and split synchronization
from draw submission when diagnosing CPU stalls.

Always report VrApi minimum FPS and stale/torn frame counts beside mean
presentation FPS. Compositor presentation at the selected refresh rate can
still reuse an old application frame; it is not evidence that every frame was
fresh. Current Horizon OS emits paired `Fov=0D` and `Fov=0` records per
interval; the benchmark uses only the primary-display `Fov=0D` series so the
compositor and app telemetry cover the same number of one-second windows.

Per-frame logging invalidates CPU measurements. Keep device logs aggregated and
structured.

## Collect Adreno counters

Quest provides `/system_ext/bin/ovrgpuprofiler`; no app instrumentation is
needed:

```sh
D=SERIAL
adb -s "$D" shell ovrgpuprofiler -m
adb -s "$D" shell 'ovrgpuprofiler --realtime="24,31,16,18,22,7,8,9,6,21,25,32,27,38"'
```

Bound realtime capture from the host and interrupt it after a settled interval.
Useful counters:

| IDs | Question |
| --- | --- |
| 24, 31 | vertex vs fragment share |
| 16, 18, 22 | shader busy, ALU use, occupancy |
| 7, 8, 9, 21 | texture stalls, L1/L2 misses, texture-pipe pressure |
| 6 | vertex-fetch stalls |
| 25, 32 | vertices and fragments per second |
| 27, 38 | textures per vertex/fragment |

For a detailed render-stage trace:

```sh
adb -s "$D" shell ovrgpuprofiler -e
# Relaunch after enabling detailed mode.
adb -s "$D" shell ovrgpuprofiler -t 1.0
adb -s "$D" shell ovrgpuprofiler -d
```

Derive per-frame work using the measured FPS:

```text
vertices/frame  = vertices/second / fps
fragments/frame = fragments/second / fps
overdraw         = fragments/frame / (2 * eye_width * eye_height)
stage_ms         = App_ms * stage_percent / 100
```

## Decide what to optimize

Evaluate each idea as an A/B on the same mission set and retain device visuals.

- **FFR:** prioritize when fragment share, texture-fetch stalls, GPU load, or
  overdraw dominate. OpenXR requires `XR_FB_foveation`,
  `XR_FB_swapchain_update_state`, the OpenGL ES swapchain-update extension, and
  foveation-capable swapchain creation. Compare off/low/medium/high, including
  peripheral visual quality.
- **Multiview:** prioritize when two per-eye timings dominate and the scene
  performs duplicate draw submission, culling, state changes, or vertex work.
  It requires texture-array swapchains/framebuffers and multiview-capable
  shaders; it does not remove genuinely eye-dependent work.
- **CPU/game update:** prioritize when `update_ms` is high even while GPU App
  time has margin. Profile physics, scripts, AI/pathfinding, visibility, and
  allocation hot paths.
- **Scene/render preparation:** prioritize when `scene_ms` or both eye CPU
  timings are high. Avoid regenerating identical eye-independent scene data and
  repeated asset/material work.
- **Resolution/material work:** prioritize only with fragment/bandwidth
  evidence. Texture-fetch stalls with low ALU use favor fewer/localized fetches;
  high fragment work favors FFR, overdraw reduction, and cheaper materials.

An exact desktop result cannot prove Quest GPU parity.

## Report

Lead with budget status and the slowest missions. Include:

1. Device, OS, APK profile/commit, refresh rate, and eye resolution.
2. Startup and steady-state tables, including minimum FPS, stale frames, torn
   frames, and skipped app frames.
3. Engine timings beside VrApi and memory.
4. GPU counter interpretation, clearly separating measurement from inference.
5. Device PNG/MP4 evidence, including failures and visual tradeoffs.
6. Ranked next steps tied to the metric that justifies each.

Include negative results; they prevent repeated dead ends.

## Finish cleanly

Disable detailed GPU mode if used and restore proximity automation:

```sh
adb -s "$D" shell ovrgpuprofiler -d
node .claude/skills/vr-device-loop/scripts/quest-device.mjs \
  --serial "$D" restore
```
