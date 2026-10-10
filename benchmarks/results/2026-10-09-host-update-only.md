# Host update-only baseline — 2026-10-09

Added a reproducible baseline for the complete simulation update without drawing. This is **host attribution**, not evidence of Quest 120 Hz or the speedup from a worker.

Source `e3011c516da1d657d0ead0952562ad0259afa7b4`; Apple M3, 16 GiB RAM, macOS 26.6.2. Optimized development build with debug assertions, `RUST_LOG=error`. Binary hash and provenance are in the [raw results](2026-10-09-host-update-only/provenance.json).

Both sequential runs use the same Many brain mixed crowd fixture, VR rest poses, upgraded terrain on, wetness off, and the opt-in fixed clock. Ten seconds of warmup precede 1,800 measured updates (30 simulation seconds). The fixture supplies six idle animated creatures and eggs; this is not an active combat stress test.

| Run | Mean update ms | p95 ms | p99 ms | Max ms | Updates >8.33 ms | Measured wall s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4.206 | 7.134 | 8.116 | 9.942 | 12 / 1800 | 30.000189 |
| 2 | 4.967 | 8.108 | 9.193 | 10.819 | 58 / 1800 | 30.001007 |

| Inclusive phase, ms/update | Run 1 | Run 2 |
| --- | ---: | ---: |
| scripts | 1.307 | 1.516 |
| physics | 1.312 | 1.522 |
| solver | 1.148 | 1.322 |
| animation | 0.621 | 0.720 |
| hitboxes | 0.322 | 0.401 |
| interaction | 0.209 | 0.291 |
| physics_sync | 0.145 | 0.200 |
| effects | 0.077 | 0.087 |
| audio | 0.014 | 0.017 |

Scripts and physics dominate this workload. `solver` is inside `physics`; do not add those values. These are elapsed timings and can include scheduling/preemption. Background host load and core placement were not controlled, so run-to-run variation is not an optimization result.

Both runs account for exactly 1,800 scene updates and 1,800 solver steps, 30.0 seconds of scene time and 30.0000016 seconds of solver time (float rounding). All six fixture creatures have advancing animation states. No visibility phase ran. The report retains every update sample and before/after simulation observations; it makes no claim about rendered mesh counts.

Graphics initialization still happens in a hidden context, and gameplay can perform lazy asset work or audio inside `Game::update`. Rendering is omitted, so GPU contention, scene preparation, visibility, stereo drawing, compositor behavior, and snapshot publication costs are absent. Quest release measurements remain necessary.

## Reproduce

```sh
CARGO_INCREMENTAL=0 cargo build -p debug_runtime
RUST_LOG=error target/debug/debug_runtime --vr \
  --benchmark-scene benchmarks/results/2026-10-09-host-update-only/fixture.json \
  --benchmark-updates 1800 --benchmark-warmup 600 \
  --benchmark-output /tmp/many-updates.json
```

Validation: all 31 debug-runtime unit tests pass, workspace formatting passes, and a separate unpaced MedSci/flat smoke run completes 120 measured scene/solver steps. The measurements above use the corrected paced implementation; preliminary sleep-overshoot runs are excluded.

Next attribution step: per-script/AI call timing and a finer split of the physics step on Quest. Before moving simulation to a worker, preserve the existing ordering of physics, scripts, effects, and animation; isolate graphics resource work; publish only completed immutable world snapshots; and keep head/controller tracking current on the render thread.
