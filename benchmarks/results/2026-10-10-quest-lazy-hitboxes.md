# Quest full query-only hitbox replacement

The complete `lazy-hitboxes` backend is compared with default Rapier limb
proxies at code revision **f4af7445**. Both are signed Android release builds
with `shock2vr/physics-profiling`; only the replacement additionally enables
`shock2vr/lazy-hitboxes`. No SIMD or proxy-omission marker is used.

Quest 3, Android 14, **90 Hz**, **1680 × 1760 per eye**, **fixed low FFR**.
The device was charging throughout. The device's installed 25AE KPF/mod archives
were retained unchanged. Many uses the authored brain-chamber camera with its
player at the mission's default start; MedSci uses the fixed cryo-room camera.
Neither scene adds enemies or drives sustained combat.

Order: default → replacement → replacement → default, Many then MedSci in each
pair. Each mission launches fresh, warms up for 30 seconds, then collects 30
one-second engine and app-PID VrApi samples. All eight samples passed focus,
refresh, FFR and per-bucket workload validation. Means below give equal weight
to the two repetitions of each variant.

## Update and physics

| Mission | Physics default → replacement | Saving | Whole update default → replacement | Saving |
| --- | ---: | ---: | ---: | ---: |
| many.mis | 1.906 → 1.594 ms | 0.312 ms / 16.4% | 5.161 → 4.552 ms | 0.609 ms / 11.8% |
| medsci1.mis | 1.323 → 1.137 ms | 0.185 ms / 14.0% | 3.982 → 3.693 ms | 0.289 ms / 7.3% |

These are measured gains for the **full replacement**, including committed
bound rebuilds and overlap polling in physics, and pose staging/gameplay queries
in the whole update. They supersede omission-only estimates for this revision
and workload; the older experiment used different code and FFR settings.

Physics is the sum of preparation, step, controller and event wrapper timings.
Rapier's individual stages are nested inside step time and are not added again.
Only complete 600-tick physics windows wholly inside the measured interval are
included. Engine/VrApi distributions describe one-second reporting buckets,
not individual-frame latency percentiles.

## Latest frame timings

| Mission | Backend | Update | Scene | Both eyes | Finish / visibility | Submit | Instrumented stage sum | Paced frame interval |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| many.mis | default | 5.161 | 1.066 | 2.146 | 0.895 | 0.365 | 9.632 | 11.153 |
| many.mis | lazy | 4.552 | 1.058 | 2.139 | 0.904 | 0.367 | 9.021 | 11.118 |
| medsci1.mis | default | 3.982 | 1.208 | 2.939 | 0.398 | 0.359 | 8.886 | 11.127 |
| medsci1.mis | lazy | 3.693 | 1.226 | 2.946 | 0.393 | 0.364 | 8.620 | 11.114 |

All entries are milliseconds. Eye timings include swapchain synchronization;
the stage sum is instrumented wall time, not pure CPU execution or a direct
prediction of 120 Hz behavior. It excludes the initial frame wait. No 120 Hz
request was made: **these measurements do not establish stable 8.33 ms frames**.
A matched 120 Hz run is required, especially because OS clock selection changes
with workload and refresh rate.

| Mission | Backend | VrApi FPS mean / minimum | App ms | CPU&GPU ms | Stale | Torn | Skipped | PSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| many.mis | default | 90.22 / 87 | 4.946 | 15.020 | 69 | 17 | 0 | 613.3 |
| many.mis | lazy | 90.52 / 89 | 4.880 | 14.258 | 37 | 19 | 0 | 613.0 |
| medsci1.mis | default | 90.47 / 89 | 2.723 | 11.872 | 59 | 1 | 0 | 725.5 |
| medsci1.mis | lazy | 90.58 / 90 | 2.688 | 11.551 | 64 | 0 | 0 | 723.7 |

Stale/torn/skipped counts total two 30-second intervals. Presentation near 90 FPS
can still contain reused application frames; it is not proof of fresh-frame
stability. VrApi App/CPU&GPU are separate telemetry metrics, not values to add
to engine timing. No Adreno stage-counter capture was needed for this CPU change.

## Repeatability and limits

| Run | Physics ms | Update ms | Physics windows | Temperature °C | Rendered objects |
| --- | ---: | ---: | ---: | ---: | --- |
| 1-default-many | 1.912 | 5.123 | 4 | 30.0 | [159, 161] |
| 1-default-medsci1 | 1.338 | 4.032 | 4 | 30.0 | [222, 227] |
| 2-lazy-many | 1.611 | 4.594 | 4 | 31.0 | [158, 161] |
| 2-lazy-medsci1 | 1.154 | 3.720 | 4 | 31.4 | [223, 227] |
| 3-lazy-many | 1.576 | 4.510 | 4 | 32.0 | [157, 160] |
| 3-lazy-medsci1 | 1.121 | 3.666 | 4 | 32.0 | [222, 227] |
| 4-default-many | 1.900 | 5.198 | 4 | 33.0 | [159, 161] |
| 4-default-medsci1 | 1.308 | 3.933 | 4 | 33.0 | [222, 227] |

Two repetitions establish a useful directional comparison, not confidence
intervals or guaranteed savings in combat. CPU/GPU frequencies were not pinned;
per-run clock records and full reporting-bucket distributions are preserved in
[summary.json](2026-10-10-quest-lazy-hitboxes/summary.json). Static framing and
fixture state were controlled, while authored animation/particles varied in
phase. Animation remains eagerly evaluated. These workloads do not stress many
simultaneous projectile or melee queries; those deserve a separate combat run.

A preliminary default-Many run was excluded when the stricter workload validator
found stale fixture metadata (it expected four Rumbler meshes outside the shot).
The final fixtures instead validate the visible brain (`Bb_brain`, one mesh)
and MedSci cryo door (`poddoor`, three meshes). Only observation expectations
changed; camera, player position and world setup were preserved. The excluded
run is retained locally and contributes to no table.

## Device evidence

Captures were inspected for the intended scene, matching static framing, intact
stereo/textures, and absence of system dialogs. Creature/particle phases differ.
They establish workload appearance, not hit-detection correctness.

| Workload | Default Rapier hitboxes | Query-only hitboxes |
| --- | --- | --- |
| Many brain chamber | ![Default Many](https://gist.githubusercontent.com/tommy-xr/57bd23877891f258728872952eff7c84/raw/default-many.png) | ![Query-only Many](https://gist.githubusercontent.com/tommy-xr/57bd23877891f258728872952eff7c84/raw/lazy-many.png) |
| MedSci cryo room | ![Default MedSci](https://gist.githubusercontent.com/tommy-xr/57bd23877891f258728872952eff7c84/raw/default-medsci1.png) | ![Query-only MedSci](https://gist.githubusercontent.com/tommy-xr/57bd23877891f258728872952eff7c84/raw/lazy-medsci1.png) |

## Reproduction and restoration

Fixtures: [Many](2026-10-10-quest-lazy-hitboxes/many-fixed.json),
[MedSci](2026-10-10-quest-lazy-hitboxes/medsci-fixed.json).
From `runtimes/oculus_runtime`, source the Android setup and use JDK 8. Build and
retain separate APKs with these feature sets:

```sh
cargo apk build --release --features shock2vr/physics-profiling
cargo apk build --release --features shock2vr/physics-profiling,shock2vr/lazy-hitboxes
```

Install each variant in the order above, push the matching fixture to
`/sdcard/shock2quest/benchmark-scene.json`, remove any hitbox-omission marker,
then run the profiling helper with `--mission`, `--warmup 30 --seconds 30`.
Validate each telemetry interval with `tools/quest-bench/run.mjs`'s
`validateWorkload`; require fixed FFR level 1 and stable 90 Hz. Both APK hashes
are recorded in the summary. The telemetry parser/workload tests passed all
15 checks. The feature itself passed 2,585 Rust tests and 16 targeted gameplay
checks before the device sweep; see [implementation notes](../../projects/lazy-hitboxes.md).

The original installed APK was restored and its SHA-256 verified. Original
mission/config files were restored byte-for-byte or removed if originally
absent. The app was stopped, Guardian pause cleared, and proximity automation
disabled. Permanent sleep settings were unchanged.

Raw logs, captures, build logs, the run script and restoration verification are
retained at `/tmp/quest-lazy-full-oct10` on the profiling host. The published
summary omits device identifiers. The default backend remains unchanged; the
replacement is still opt-in.
