# Quest 3 object-lighting baseline — 2026-09-21

The six-hybrid workload presents near 90 Hz but reuses about 3% of frames. Object lighting adds 0.33 ms to mean VrApi App time in that workload; the two-repeat sample does not establish a frame-pacing regression or improvement. The simpler lit corpse view adds 0.62 ms App time. Scene preparation changes are small (0.02–0.04 ms).

## Setup

- Quest 3, Android 14, OS build `52433670036000520`.
- Release APK SHA-256: `85581edb16203febd4a6fc338aa521897b195d81b17de7f0c2f1b95dc82b94e9`.
- Source base: `44d216aa` (`feat/player-object-lighting`) plus the benchmark implementation. The preserved performance APK predates the final review changes to diagnostic subject identity and camera-midpoint reuse; final fixtures are separately smoke-tested after rebuilding.
- Final rebuilt APK `aed0b98374be8a45450d0afd58bd54765a24a2946891d9c24c322788cf6e939c` passed all three fixtures with lighting on (5 s warmup, 5 s verification each). These smoke samples are excluded from the performance table.
- 90 Hz requested and active throughout; 1680 × 1760 per eye.
- Same APK for every arm. Each scene ran off/on/on/off, with 10 s warmup and 30 s sampling after each restart. Two runs (60 s total) per table row.
- Battery temperature 43–44 °C; battery level 60% to 53%. Android reports AC powered throughout (USB powered false); snapshots are retained with the raw runs. Clocks were not locked, so treat small differences cautiously.
- All 12 runs passed release/focus/refresh checks, expected mesh counts, lamp states, and creature animation advancement. Raw telemetry was revalidated against the stricter timing/workload pairing and rolling animation checks.
- Subjects: two corpse meshes in each static view; six live hybrids emitting twelve meshes in the stress view. The authored Rumbler is removed.
- Observation runs once per second outside the measured engine stages. Total frame pacing includes diagnostic/logging overhead in both arms; these are instrumented workloads, not an uninstrumented shipping-build benchmark.

## Paired results

Times are means in milliseconds across two equal-length runs. Minimum FPS is the lowest one-second VrApi reading; stale/torn/skipped are totals over 60 seconds. Eye time includes swapchain synchronization and is not pure GPU time.

| Scene | Object lighting | Update | Scene prep | Both eyes | Finish | VrApi App | FPS mean/min | Stale/torn/skipped | GPU load | PSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| rec1-court-lit | off | 4.669 | 0.530 | 1.534 | 1.272 | 1.744 | 90.53/90 | 25/0/0 | 0.248 | 600.5 |
| rec1-court-lit | on | 4.635 | 0.550 | 1.675 | 1.278 | 2.366 | 90.53/90 | 61/3/0 | 0.304 | 600.8 |
| rec1-court-dark | off | 4.635 | 0.492 | 1.529 | 1.266 | 2.207 | 90.52/89 | 34/1/0 | 0.279 | 600.5 |
| rec1-court-dark | on | 4.525 | 0.535 | 1.554 | 1.268 | 2.158 | 90.57/90 | 26/0/0 | 0.275 | 600.3 |
| rec1-six-hybrids | off | 4.611 | 0.943 | 2.369 | 1.328 | 2.389 | 89.80/87 | 187/0/0 | 0.302 | 613.7 |
| rec1-six-hybrids | on | 4.534 | 0.980 | 2.439 | 1.325 | 2.717 | 89.93/88 | 179/0/0 | 0.332 | 613.0 |

Startup `Game::init` means range from 4.64–4.71 s; launch-to-focused means range from 5.67–5.90 s. Fixture placement and its two-second lamp settling phase are outside Game initialization and inside warmup.

## Individual runs

| Scene | Flag | Repeat | Scene prep ms | App ms | FPS mean/min | Stale | Torn |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| rec1-court-dark | off | 1 | 0.491 | 2.311 | 90.433/89 | 19 | 1 |
| rec1-court-dark | on | 1 | 0.542 | 2.083 | 90.467/90 | 13 | 0 |
| rec1-court-dark | on | 2 | 0.528 | 2.232 | 90.667/90 | 13 | 0 |
| rec1-court-dark | off | 2 | 0.494 | 2.102 | 90.600/90 | 15 | 0 |
| rec1-court-lit | off | 1 | 0.516 | 1.854 | 90.500/90 | 7 | 0 |
| rec1-court-lit | on | 1 | 0.567 | 2.420 | 90.533/90 | 44 | 3 |
| rec1-court-lit | on | 2 | 0.533 | 2.312 | 90.533/90 | 17 | 0 |
| rec1-court-lit | off | 2 | 0.543 | 1.635 | 90.567/90 | 18 | 0 |
| rec1-six-hybrids | off | 1 | 0.918 | 2.362 | 90.000/88 | 77 | 0 |
| rec1-six-hybrids | on | 1 | 0.979 | 2.809 | 89.967/88 | 96 | 0 |
| rec1-six-hybrids | on | 2 | 0.981 | 2.625 | 89.900/89 | 83 | 0 |
| rec1-six-hybrids | off | 2 | 0.967 | 2.415 | 89.600/87 | 110 | 0 |

## Separate GPU-counter sample

A second off/on pass used the same performance APK and six-hybrid fixture, with
10 s warmup followed by a 20 s profiler capture (18 complete counter groups per
arm). These counters are separate from the 12 timing runs above. Metric IDs were
discovered from this device's `ovrgpuprofiler -m`; they differ from older devices.

| Counter | Off | On |
| --- | ---: | ---: |
| GPU frequency (MHz) | 456 | 466 |
| GPU utilization (%) | 20.48 | 21.69 |
| Shader busy (%) | 68.91 | 70.44 |
| Fragment shading share (%) | 83.67 | 84.77 |
| Vertex shading share (%) | 16.34 | 15.23 |
| Texture-fetch stall (%) | 4.07 | 3.64 |
| Vertices shaded / second | 33.74 million | 33.71 million |
| Fragments shaded / second | 1.373 billion | 1.319 billion |

Fragment work dominates the reported shading share, but overall GPU utilization
has substantial headroom in these samples. This does not establish a GPU-bound
workload or justify choosing foveation before investigating CPU/frame pacing.
Clock changes and separate animated runs prevent attributing every counter delta
to object lighting. Raw counter output and focused device captures are under
`/tmp/quest-object-lighting/gpu/`.

## Visual evidence

Both eyes were inspected in the accepted captures. In the lit room, the corpse and hybrids receive authored lighting; in the dark room the corpse becomes much darker. This establishes rendered behavior, not wearer comfort or dark-room readability. Creature poses differ between restarted runs.

![Quest stereo comparisons](https://gist.githubusercontent.com/tommy-xr/107af9dbf971496450f9a2731997e5a6/raw/522ae0d2d4449fe3109d9f09c68134adb96ae6c2/quest-lighting-ab.png)

## Excluded trials and limits

- Default-pose MedSci1 samples faced a nearby wall and are excluded.
- Early Rec1 fixtures applied their lamp state before placement-triggered mission events settled. Fifteen lamps returned to off, making the supposedly lit room flat ambient gray. Those samples are excluded. Waiting two seconds after placement, then applying lamp state once, fixed the captured workload. Lamp-state validation now catches recurrence.
- Shader precision and atlas-wrapping experiments were discarded; no shader change is part of this benchmark implementation.
- The first profiler attempt lost its focus marker to the 256 KiB Android log ring. The runner temporarily uses 16 MiB and restores the previous size.
- These scenes are a starter set, not proven worst-case mission views. They do not cover combat, particles, held-weapon/glove close-ups, or transitions.
- At two repeats per arm, with unlocked clocks and 43–44 °C battery temperature, small timing differences and changes in stale counts need more repetition before attribution.

## Next measurements

1. Use the six-hybrid scene to investigate frame reuse; update costs ~4.5–4.6 ms and both-eye CPU wall time ~2.4 ms. Separate synchronization from draw submission before selecting an optimization.
2. Survey real mission views and retain a portal-heavy room and a particle-heavy encounter. Label synthetic stress separately from normal gameplay.
3. Add a controlled glove/held-weapon close-up and obtain wearer feedback on dark-room readability.

Raw local artifacts: `/tmp/quest-object-lighting/baseline-v1/` (per-run fixtures, results, telemetry, battery snapshots and stereo captures). Reproduce with `node tools/quest-bench/run.mjs --scene all --lighting both --repeats 2 --warmup 10 --seconds 30`.
