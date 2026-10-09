# Quest 3: Many brain-room terrain comparison — 2026-10-09

The 25AE terrain upgrade has no clear performance regression in this short paired sample. Wetness adds measurable fragment/texture work. All three modes occasionally miss the 90 Hz budget; none demonstrates a sustained fresh 90 FPS in this crowded scene.

## Setup

- Quest 3, Android 14; release APK (no DEBUGGABLE package flag).
- APK SHA-256: `c65e0276400c40de6689d650cfa74395b812ac84a1a8a475fd05ea16ecd4ca2c`.
- Runtime source: `85af0d32`; `5997881d` is import formatting only. Exact measurement provenance is retained in `results.json`.
- OS build: `oculus/eureka/eureka:14/UP1A.231005.007.A1/52433670048800520:user/abl_signing_keys:release,amss_signing_keys:release,release-keys`.
- Requested and active refresh: 90 Hz; 1680 × 1760 per eye, unchanged across all six runs.
- Same APK and matching host/device KPF hashes. Object assets and object lighting remain identical; classic means the terrain flag is off, not all remaster assets off.
- Fixed camera and world spotlight in SCP `many.mis`; four added rumblers, two overlords, four floor eggs, plus authored entities. Player remains at the entrance; no save or ending trigger is used.
- Six runs: classic, upgraded, wet, wet, upgraded, classic. Each has 10 s warmup and 30 s measured telemetry. Ten seconds of device-wide GPU counters follow each timing/visual sample, separately.
- Every timing bucket passed mode, lighting, mesh-count, and skeletal-animation validation. Counts: 4 rumbler, 6 overlord, 8 egg meshes; six added animated creatures. Eggs are static. All sessions focused; no skipped application frames.
- AI/physics remain live; this is a synthetic stress scene. Creature poses/particles can differ between launches. Two repeats are enough for an initial comparison, not a confidence interval or thermal-soak result.

## Combined results

Each row combines two 30-second samples. FPS minimum is the worst one-second bucket; stale/torn are totals across 60 seconds. PSS is the mean of two post-sample snapshots.

| Terrain | FPS mean / min | VrApi App ms | GPU load | PSS MiB | Stale / torn |
| --- | ---: | ---: | ---: | ---: | ---: |
| classic | 89.00 / 83 | 8.495 | 84.7% | 639.1 | 105 / 20 |
| upgraded | 89.00 / 84 | 8.599 | 85.5% | 592.7 | 123 / 19 |
| wet | 87.72 / 83 | 8.827 | 85.6% | 593.5 | 206 / 5 |

25AE versus classic: App time +0.105 ms, mean FPS +0.00, PSS -46.4 MiB. Wetness versus upgraded: App time +0.228 ms, mean FPS -1.28. Small timing differences overlap run-to-run variation; lower observed PSS is not a general claim that higher-detail textures save memory.

| Terrain | Update ms | Scene ms | Both eyes ms | Finish ms |
| --- | ---: | ---: | ---: | ---: |
| classic | 6.247 | 0.713 | 2.245 | 1.532 |
| upgraded | 6.275 | 0.703 | 2.278 | 1.525 |
| wet | 6.287 | 0.744 | 2.377 | 1.551 |

Engine columns are CPU wall time, not GPU time; eye timing includes swapchain synchronization. VrApi FPS measures presentation and can include reused frames—stale/torn counts are therefore shown explicitly.

## Individual runs

| Order | Mode | FPS mean / min | App ms | Init ms | Focused ms | Stale / torn | PSS MiB | VrApi temp °C |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | classic 1 | 88.833 / 83 | 8.522 | 4779 | 7508 | 56 / 9 | 638.7 | 29.5 |
| 2 | upgraded 1 | 89.167 / 86 | 8.575 | 5055 | 6379 | 58 / 5 | 593.1 | 32.2 |
| 3 | wet 1 | 88.433 / 84 | 8.726 | 5093 | 6290 | 79 / 1 | 593.3 | 35.0 |
| 4 | wet 2 | 87.000 / 83 | 8.928 | 5066 | 6138 | 127 / 4 | 593.7 | 38.1 |
| 5 | upgraded 2 | 88.833 / 84 | 8.624 | 5066 | 6193 | 65 / 14 | 592.2 | 39.2 |
| 6 | classic 2 | 89.167 / 85 | 8.467 | 4798 | 5909 | 49 / 11 | 639.5 | 40.0 |

Battery snapshots are retained alongside telemetry. The headset stayed connected to power. Warm-up, OS scheduling, adaptive clocks, and thermal drift remain possible sources of small differences.

## GPU counters

The second classic GPU interval contains unavailable `-1` counters and is excluded in full. Classic GPU values therefore use one interval; upgraded/wet use two. This correction does not change the separately measured FPS, App, CPU, or memory results. The runner now rejects negative counter sentinels.

Counters use names discovered from this OS; the numbered IDs differ from older profiling notes. These are device-wide samples, including compositor work, and were collected after FPS measurement.

| Terrain | Fragment share | Shaders busy | Texture fetch stall | Textures / fragment | Fragments / second |
| --- | ---: | ---: | ---: | ---: | ---: |
| classic (run 1 only) | 89.1% | 86.1% | 1.25% | 1.664 | 1.481 billion |
| upgraded | 89.9% | 86.3% | 2.82% | 1.656 | 1.464 billion |
| wet | 91.2% | 87.9% | 7.82% | 2.552 | 1.554 billion |

Wetness increases texture work even where its visible highlight is tiny. Fragment shading dominates these GPU samples; FFR or reduced fragment work is a reasonable next experiment. CPU update/visibility also consumes much of the frame budget, so GPU optimization alone is not established as sufficient for fresh 90 FPS.

## Visual evidence

Identical desktop fixture, fixed-timestep sequence, camera, and light. Walls/floor change; the brain and creature object assets are the same in both modes. AI can select slightly different idle poses.

![Classic versus upgraded 25AE terrain](https://gist.githubusercontent.com/tommy-xr/5cfa2e37b98705b28d963f8e96c80bad/raw/terrain-comparison.gif)

![Full-resolution before/after](https://gist.githubusercontent.com/tommy-xr/5cfa2e37b98705b28d963f8e96c80bad/raw/terrain-comparison-t0.png)

Unmodified Quest stereo captures, visually inspected for the brain/crowd in both eyes:

- [Classic](https://gist.githubusercontent.com/tommy-xr/5cfa2e37b98705b28d963f8e96c80bad/raw/quest-classic-stereo.png)
- [Upgraded 25AE](https://gist.githubusercontent.com/tommy-xr/5cfa2e37b98705b28d963f8e96c80bad/raw/quest-upgraded-stereo.png)
- [25AE + wetness 1.5](https://gist.githubusercontent.com/tommy-xr/5cfa2e37b98705b28d963f8e96c80bad/raw/quest-wet-stereo.png)

The visible OMW walls/floor and OVM005 faces are wetness-eligible, and the GPU counters confirm extra shader work. However, sampled stationary wall/floor patches show no perceptible wet highlight at this fixed light/view angle. This is a cost test, not a wetness showcase; use the spotlight sweep in PR #2118 to judge that appearance.

## Reproduce and inspect

```sh
node tools/quest-bench/run.mjs --scene many-brain-mixed-crowd \
  --lighting on --terrain all --repeats 2 --warmup 10 --seconds 30 \
  --gpu-seconds 10 --output /tmp/quest-many-terrain
```

[Machine results and per-second workload evidence](2026-10-09-quest3-many-terrain/results.json), [counter mapping](2026-10-09-quest3-many-terrain/gpu-metrics.txt), and per-run fixture/telemetry/GPU/battery text files live in the adjacent directory. Device serial identifiers are omitted.

Validation: release APK build/install, three Rust fixture tests, 25 Node tests covering device/telemetry/runner behavior, workspace formatting check, six valid hardware samples, and matched desktop GIF/PNG captures. The runner stopped the app and restored benchmark config, mission selection, log buffer, Guardian/proximity settings; detailed GPU mode was not enabled.
