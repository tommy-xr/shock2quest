# Quest 3: fixed scene-clock validation (2026-10-09)

The opt-in clock corrects the measured solver-time drift while preserving the existing 1/60-second integration step. This is a correctness result, not evidence of stable 120 Hz. World/physical hand presentation still uses the most recent 60 Hz tick without interpolation; the legacy path remains the default.

## Method

- Release APK source `ae7fa5a3`, SHA-256 `5e407422c71ca5c13b8dab7714abaaabfb107d076bf16f6a72552f5e7fc535d4` for all four runs.
- Same Many brain crowd, upgraded terrain, wetness off, object lighting on, fixed low FFR, 90 Hz, 1680×1760 per eye.
- Legacy/fixed/fixed/legacy order, 15 s warmup and 30 s measured per run. CPU profiling enabled throughout; no GPU counter collection.
- All four workload/focus/refresh/FFR validations passed. Fixed runs additionally require solver time within one tick of independently accumulated active elapsed time.

## Clock and frame delivery

| Run | Active s | Solver s | Solver/active | FPS mean | App ms | Update ms | Stale | Temp °C |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| legacy A1 | 30.178 | 40.400 | 1.338741 | 81.00 | 8.230 | 6.435 | 323 | 39.7 |
| fixed B1 | 30.161 | 30.150 | 0.999633 | 81.63 | 8.483 | 4.913 | 340 | 42.8 |
| fixed B2 | 30.163 | 30.167 | 1.000117 | 83.10 | 8.276 | 4.785 | 320 | 44.4 |
| legacy A2 | 30.214 | 40.933 | 1.354796 | 82.30 | 8.088 | 6.448 | 291 | 46.3 |

FPS and App/update figures summarize the 30 one-second buckets. Stale counts cover each measured interval. Temperature and workload variation still limit a small two-repetition performance comparison.

## CPU phases and tails

| Run | Scripts ms/tick | Physics ms/tick | Animation ms/tick | Visibility ms/call | Frame-wall p95 upper ms | Frames >8.33 ms |
|---|---:|---:|---:|---:|---:|---:|
| legacy A1 | 2.270 | 1.769 | 0.831 | 1.562 | 14.0 | 2424/2424 |
| fixed B1 | 2.357 | 1.834 | 0.817 | 1.547 | 14.2 | 1809/2452 |
| fixed B2 | 2.323 | 1.799 | 0.813 | 1.570 | 14.0 | 1810/2486 |
| legacy A2 | 2.292 | 1.778 | 0.823 | 1.553 | 13.9 | 2456/2456 |

Phase means are weighted by calls and are inclusive (physics contains solver work); do not sum nested phases. The frame-wall histogram spans after `wait_frame` through `end_frame`, including synchronization, so it is not pure CPU execution time. Its p95 is an upper bound from 100 µs bins, not a percentile of one-second means.

## Validation and remaining gates

- Six host clock/physics tests pass: equal tick counts, retained remainder and bounded catch-up debt, omitted paused time, identical falling/movement and full jump trajectories across 30/60/72/90/120/144 Hz.
- Three input-buffer tests cover sampled press/release sequences and repeated actions across zero-tick frames, no duplicate actions during catch-up, queued click coordinates, current tracking poses and analog noise. Three profiling tests and 14 Node benchmark tests pass; workspace format check passes.
- Device pause/resume results are retained in the evidence directory: paused reports contain zero scene updates and solver steps while rendered CPU frames continue; resume remains within one tick of active elapsed time.
- The rebuilt APK also confirms low FFR with no override file and legacy clock mode when no clock file/fixture exists.
- Still required before promoting fixed mode: focus-loss/device lifecycle verification, transitions/loading, save/replay round-trips, doors/platforms, ragdolls/projectiles, held-item contacts/throwing/haptics, and wearer assessment of 60 Hz world/hand presentation or interpolation. Refresh remains 90 Hz.

[Raw telemetry, fixtures, asset/APK provenance and pause evidence](2026-10-09-quest3-fixed-clock/results.json). Device serials are redacted.

## Visual comparison

The same-build stereo captures retain the intended scene in both eyes. Live animation poses differ. This alternating-still GIF checks rendering continuity; it is not motion footage and cannot establish smoothness or interaction latency.

![Clock comparison](https://gist.githubusercontent.com/tommy-xr/ec68f787c907d4193ea66c806a61e22f/raw/clock-still-comparison.gif)

![Stereo before and after](https://gist.githubusercontent.com/tommy-xr/ec68f787c907d4193ea66c806a61e22f/raw/clock-before-after.png)

## Reproduce

```sh
node tools/quest-bench/run.mjs --scene many-brain-mixed-crowd \
  --lighting on --terrain upgraded --ffr low --fixed-simulation \
  --repeats 2 --warmup 15 --seconds 30 --gpu-seconds 0 \
  --output /tmp/quest-clock-fixed
```

For the control, replace `--fixed-simulation` with `--profile-cpu`. For normal Quest interaction testing, write `fixed` to `/sdcard/shock2quest/simulation-clock.txt` and restart; remove it or write `legacy` to restore the default.
