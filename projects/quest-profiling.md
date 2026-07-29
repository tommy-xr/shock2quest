# Quest profiling

## Initial mission-spawn baseline

Measured July 28, 2026 on a Quest 3 running Android 14 with a release APK from
this change. Each mission was launched in a fresh process, allowed to warm up
for three seconds after reaching the OpenXR `FOCUSED` state, then sampled for
five seconds. The runtime rendered at 1680x1760 per eye and 90 Hz. Human review
accepted identifiable mission content for 19 captures. `eng1.mis`, `eng2.mis`,
`hydro3.mis`, and `ops4.mis` produced valid focused telemetry but only
environment/void frames, including direct focused recapture attempts.

This is a stationary initial-spawn baseline. It proves that every mission
loads, reaches a focused XR session, and submits stereo frames at its spawn. It
does not represent combat, populated sight lines, effects-heavy rooms, or a
thermally settled long play session. Compositor stale-frame counts also mean
the near-90 Hz presentation rate must not be interpreted as every frame being
fresh application work.

| Mission | Game init | Focused | FPS mean/min | Stale | App | Update | Scene | Both eyes | GPU load | PSS | Visual |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| command1.mis | 3405.4 ms | 4658 ms | 90.000/89 | 7 | 3.940 ms | 2.679 ms | 0.328 ms | 3.663 ms | 0.498 | 370.4 MiB | pass |
| command2.mis | 5004.8 ms | 6490 ms | 90.200/89 | 4 | 2.238 ms | 2.994 ms | 0.352 ms | 3.874 ms | 0.340 | 410.9 MiB | pass |
| earth.mis | 2346.6 ms | 3904 ms | 90.400/90 | 2 | 5.514 ms | 1.495 ms | 0.769 ms | 4.129 ms | 0.640 | 345.2 MiB | pass |
| eng1.mis | 5056.9 ms | 6513 ms | 90.400/90 | 5 | 4.480 ms | 3.208 ms | 0.401 ms | 4.316 ms | 0.536 | 405.8 MiB | environment/void |
| eng2.mis | 3779.5 ms | 5228 ms | 89.800/88 | 14 | 5.090 ms | 3.541 ms | 0.343 ms | 3.312 ms | 0.600 | 384.2 MiB | environment/void |
| hydro1.mis | 2180.3 ms | 3462 ms | 90.600/90 | 5 | 2.476 ms | 2.043 ms | 0.609 ms | 4.000 ms | 0.364 | 351.9 MiB | pass |
| hydro2.mis | 4360.1 ms | 5607 ms | 90.000/89 | 9 | 2.242 ms | 3.173 ms | 0.486 ms | 4.251 ms | 0.340 | 410.7 MiB | pass |
| hydro3.mis | 1453.8 ms | 3039 ms | 90.200/90 | 1 | 1.630 ms | 1.153 ms | 0.371 ms | 2.282 ms | 0.284 | 328.4 MiB | environment/void |
| many.mis | 4007.5 ms | 5660 ms | 90.400/90 | 5 | 1.826 ms | 3.527 ms | 0.352 ms | 3.241 ms | 0.302 | 381.2 MiB | pass |
| medsci1.mis | 4634.3 ms | 6065 ms | 90.800/90 | 5 | 2.340 ms | 2.861 ms | 0.487 ms | 4.104 ms | 0.348 | 416.8 MiB | pass |
| medsci2.mis | 4264.9 ms | 5569 ms | 90.400/90 | 3 | 2.510 ms | 2.619 ms | 0.328 ms | 3.710 ms | 0.370 | 401.6 MiB | pass |
| ops1.mis | 790.6 ms | 2128 ms | 90.800/90 | 1 | 3.866 ms | 0.899 ms | 0.298 ms | 1.400 ms | 0.488 | 312.6 MiB | pass |
| ops2.mis | 3255.7 ms | 4716 ms | 90.600/90 | 7 | 2.356 ms | 2.475 ms | 0.490 ms | 3.999 ms | 0.350 | 383.3 MiB | pass |
| ops3.mis | 2640.0 ms | 3892 ms | 90.600/90 | 7 | 2.198 ms | 2.534 ms | 0.453 ms | 3.663 ms | 0.330 | 380.0 MiB | pass |
| ops4.mis | 2772.2 ms | 4251 ms | 90.400/90 | 4 | 1.148 ms | 2.998 ms | 0.248 ms | 1.315 ms | 0.240 | 371.8 MiB | environment/void |
| rec1.mis | 4280.2 ms | 5619 ms | 90.400/90 | 8 | 2.406 ms | 2.875 ms | 0.362 ms | 3.864 ms | 0.360 | 402.9 MiB | pass |
| rec2.mis | 3790.9 ms | 5157 ms | 90.600/90 | 5 | 1.752 ms | 2.399 ms | 0.376 ms | 3.908 ms | 0.304 | 384.0 MiB | pass |
| rec3.mis | 3211.5 ms | 4764 ms | 89.600/89 | 7 | 2.244 ms | 2.364 ms | 0.363 ms | 3.986 ms | 0.342 | 371.6 MiB | pass |
| rick1.mis | 4618.7 ms | 6011 ms | 90.400/90 | 15 | 2.174 ms | 3.356 ms | 0.554 ms | 4.838 ms | 0.332 | 417.2 MiB | pass |
| rick2.mis | 1202.9 ms | 2583 ms | 90.200/90 | 1 | 1.462 ms | 1.528 ms | 0.313 ms | 1.847 ms | 0.272 | 329.7 MiB | pass |
| rick3.mis | 2333.5 ms | 3920 ms | 90.400/90 | 2 | 3.130 ms | 1.446 ms | 0.365 ms | 3.097 ms | 0.420 | 340.1 MiB | pass |
| shodan.mis | 2049.6 ms | 3438 ms | 90.600/90 | 0 | 1.608 ms | 1.314 ms | 0.304 ms | 2.681 ms | 0.282 | 332.4 MiB | pass |
| station.mis | 2272.4 ms | 3844 ms | 90.200/89 | 4 | 2.262 ms | 1.875 ms | 0.658 ms | 3.955 ms | 0.342 | 350.0 MiB | pass |

`Game init` measures `Game::init`. `Focused` is host-observed activity launch
through both the first submitted XR frame and the OpenXR `FOCUSED` transition.
Engine stage values are means of five one-second focused windows. Compositor
values use the last five primary-display (`Fov=0D`) records; current Horizon OS
also emits a paired `Fov=0` line that is not a second independent sample. `App`,
presentation rate, stale frames, and GPU load are independent compositor
telemetry. `Both eyes` is the sum of sequential left- and right-eye wall times;
GPU execution can overlap that CPU interval. The app profiler reported zero
skipped render frames for every mission. VrApi reported zero torn frames in
this run.

### Findings

- All 23 missions loaded, reached `FOCUSED`, and emitted complete app and
  compositor telemetry. Mean presentation stayed close to 90 Hz, but 22
  missions reported 1-15 stale frames during the five-second sample.
  `eng2.mis` had the lowest observed interval at 88 FPS; `rick1.mis` had the
  most stale frames at 15. This short sample establishes a regression baseline,
  not flawless frame delivery.
- The highest mean compositor app time was `earth.mis` at 5.514 ms, followed by
  `eng2.mis` at 5.090 ms and `eng1.mis` at 4.480 ms. These stationary spawns
  retain apparent headroom inside the 11.11 ms 90 Hz interval, but longer
  thermally settled gameplay samples are needed to explain the stale frames.
- Startup is the clearest measured issue. `eng1.mis`, `medsci1.mis`,
  `rick1.mis`, and `command2.mis` take 6.01-6.51 seconds to reach the first
  submitted frame and focused session. `Game::init` accounts for 4.62-5.06
  seconds of those launches.
- The highest mean update values are `eng2.mis` (3.541 ms), `many.mis`
  (3.527 ms), `rick1.mis` (3.356 ms), `eng1.mis` (3.208 ms), and
  `hydro2.mis` (3.173 ms).
- The largest mean combined eye-render time is `rick1.mis` at 4.838 ms,
  followed by `eng1.mis` at 4.316 ms and `hydro2.mis` at 4.251 ms.
- Total PSS ranges from 312.6 MiB to 417.2 MiB. `rick1.mis`, `medsci1.mis`,
  `command2.mis`, and `hydro2.mis` are the largest stationary spawns.
- Human review validated identifiable mission content for 19 missions. Direct
  focused recaptures recovered `command1.mis`, `earth.mis`, and `ops1.mis`, but
  `eng1.mis`, `eng2.mis`, `hydro3.mis`, and `ops4.mis` still showed only the
  environment/void. Their numeric telemetry remains valid because the runtime
  independently reported a focused, advancing session. This is the concrete
  reason a raw-eye capture endpoint belongs in the next device-control
  increment; do not use those four rows for render-quality or render-cost
  comparisons until a raw-eye capture confirms the submitted workload.

## Optimization decisions

### Fixed foveated rendering

The headset runtime reports support for `XR_FB_foveation`,
`XR_FB_foveation_configuration`, `XR_FB_swapchain_update_state`, and the
OpenGL ES swapchain-update extension. FFR is therefore implementable, but the
stationary baseline does not yet demonstrate a frame-budget failure.

Capture Adreno vertex/fragment share, texture stalls, and overdraw on
`earth.mis`, `eng2.mis`, `rick3.mis`, and an effects-heavy gameplay scenario
before prioritizing it. If those workloads are fragment-bound, compare
off/low/medium/high with matched raw-eye and compositor visuals. Prefer dynamic
FFR only after fixed levels establish the quality/performance curve.

### Multiview

The runtime currently creates two independent single-layer swapchains and runs
`game.render_per_eye` plus engine submission sequentially for each eye.
Multiview can remove duplicated draw submission, state changes, and vertex
work, so it is the stronger architectural candidate even though the stationary
spawn fits budget. It requires:

1. a texture-array OpenXR swapchain and multiview framebuffer;
2. multiview-capable vertex shaders and eye-indexed view/projection data;
3. separation of eye-independent scene preparation from genuinely
   eye-dependent HUD/hand work;
4. an A/B benchmark over the same missions and scripted gameplay workloads.

Adreno counters should determine whether the expected gain is mainly CPU draw
submission, vertex work, or both.

### Other measured targets

1. Add staged startup spans around mission parse/merge, asset decode/upload,
   entity instantiation, physics creation, and OpenXR/EGL setup.
2. Profile update systems on `rick1`, `many`, `eng2`, and `hydro2`, starting
   with scripts, physics, AI/pathfinding, visibility, and allocation counts.
3. Split `render_per_eye`, engine submission, GPU execution, and
   `finish_render` more precisely; the second-eye wall time currently includes
   end-of-frame work.
4. Add deterministic device scenarios for locomotion, combat, particle-heavy
   rooms, and populated sight lines, with longer thermally settled samples.
5. Track peak and retained asset memory, not only process PSS.

## Quest debug-runtime direction

Quest automation still needs a visible, focused OpenXR session; “headless”
means unattended and ADB-driven rather than background rendering. Add a thin
loopback HTTP server and reach it with `adb forward`, reusing the desktop debug
runtime's wire types:

1. `GET /v1/info` and `GET /v1/metrics` for mission, XR session state, frame
   counter, views, and aggregate timings;
2. `POST /v1/screenshot` for undistorted left/right swapchain images;
3. existing discrete input actions and continuous control channels;
4. mission transitions and deterministic scripted benchmark scenarios;
5. shared entity/physics inspection after extracting a runtime-neutral
   interface.

The server must report an idle or unfocused XR session explicitly and reject
stale captures. This makes a loopback API useful for debugging without
pretending Quest rendering is independent of headset lifecycle.

## OpenXR fork removal

The Oculus runtime still depends on the repository's OpenXR 0.16 fork. That
fork originally supplied OpenGL ES and Meta extensions that are present in
current upstream `openxr`. Migrate to current crates.io OpenXR in a separate,
small PR:

1. switch the dependency and adapt compile-time API changes;
2. build, install, and verify session focus, tracking, input, swapchains, and
   all 23 mission spawns on device;
3. compare startup, frame telemetry, memory, and device visuals against this
   baseline;
4. remove `vendor/openxrs` only after the device comparison passes.

Keeping the dependency migration separate makes the large vendored deletion
reviewable and preserves this baseline as a clean comparison point.

## Reproduce

From the repository root with a release APK installed and a Quest attached:

```sh
node --test \
  .claude/skills/oculus-profiling/scripts/quest-benchmark.test.mjs

node .claude/skills/oculus-profiling/scripts/quest-benchmark.mjs \
  --all --warmup 3 --seconds 5 --output /tmp/quest-all-levels
```

Use a longer warmup and interval for optimization decisions. The short sweep
above was selected to establish device interaction and cover every mission
without claiming a thermally stable performance certification.
