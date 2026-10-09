# Quest benchmark scenes

Named workloads make renderer changes comparable without relying on where a
resting headset happens to point. These are real mission views with explicit
player/camera positions, authored lamp states, and optional synthetic creatures.
They are a starter set for object lighting, **not yet a survey of the game's
worst-performing locations**.

| Scene | Workload |
| --- | --- |
| `rec1-court-lit` | Court balcony corpse, 16 authored lamps switched on |
| `rec1-court-dark` | Identical view with those lamps switched off |
| `rec1-six-hybrids` | Six animated hybrids on the court, authored Rumbler removed |

Build/install a release Quest APK containing the benchmark runtime support, then:

```sh
node --test .claude/skills/vr-device-loop/scripts/quest-device.test.mjs \
  .claude/skills/oculus-profiling/scripts/quest-benchmark.test.mjs \
  tools/quest-bench/run.test.mjs
node tools/quest-bench/run.mjs --scene all --lighting both \
  --repeats 2 --warmup 10 --seconds 30 --output /tmp/quest-lighting
```

Requires Node 22+, `adb`, installed game assets, and an attached authorized Quest.
`--scene NAME` selects one fixture. `--lighting off|on|both` controls only the
experimental object-lighting feature; authored lamps are part of each scene.
Each repeat restarts the same installed APK. Two paired repeats run off/on/on/off
to reduce ordering bias. The existing Oculus profiler supplies release-build,
focused-session, refresh-rate and complete-sample checks, stage timings, VrApi
telemetry, memory, and a stereo PNG. Inspect those PNGs before accepting results.

The runner writes the exact fixture beside each result. Once-per-second workload
records must show every expected subject mesh and the expected lighting path;
synthetic creatures must also have advancing animation states. Lamp intensities
must match the fixture throughout the interval. Placement settles for two seconds
before lamp setup, allowing teleport-triggered mission events to finish first;
the minimum warmup is three seconds. Failed runs are
excluded and cause a nonzero exit. `results.json` retains failures and raw sample
evidence rather than silently averaging them into a comparison. Battery/power
snapshots help identify thermally incomparable runs. Engine eye timings include
swapchain synchronization; they are not GPU timings. Always compare minimum FPS
and stale/torn frames alongside mean FPS.

Setup is opt-in through `/sdcard/shock2quest/benchmark-scene.json`. The runtime
fails loudly on malformed fixtures. Removing this file restores ordinary launch
behavior. The runner restores its previous contents, the profiler restores the
mission selector, and cleanup stops the app and restores Guardian/proximity and
the logcat ring-buffer size. It does not install or replace an APK itself.

Mesh counts describe scene preparation, not proof of visible GPU pixels; the
stereo PNG remains a required visual check.

The fixed camera compensates the live headset pose while retaining stereo eye
offsets. Use these views for unattended measurement; remove the fixture before
ordinary headset play. Physics, scripts and animation continue running, so these
are repeatable workloads rather than bit-identical simulations. Compare repeats
and inspect workload records for drift.

## Extending the set

Add a JSON file under `scenes/` with a stable name, a data-relative mission,
player position, camera eye/look-at, and the expected model/mesh count. Authored
object IDs go in `light_templates` or `remove_templates`; runtime entity IDs are
resolved afresh. `spawns` uses negative gamesys template IDs and world positions.
Verify a new fixture on device before treating its numbers as a baseline.

Useful next additions are a broad portal-heavy room, dense animated creatures,
translucent particles, and a held-weapon/glove close-up. Select heavy real-mission
views using an initial mission sweep, then retain the ones that stress different
measured bottlenecks. Keep synthetic workloads separate from claims about normal
gameplay performance.

## Baselines

- [Quest 3 refresh-rate exploration, 2026-10-06](results/2026-10-06-quest3-refresh.md): 90/120 Hz crowd samples, lighting cost, and PR #2068 lifecycle checks, with retained telemetry.
- [Quest 3 object lighting, 2026-09-21](results/2026-09-21-quest3-lighting.md): paired timing runs, GPU counters, and stereo comparisons.

## Many brain room: classic terrain versus 25AE

`many-brain-mixed-crowd` places a fixed camera in the SCP version of the Many's
brain room. The player stays at the mission entrance to avoid the ending
trigger. Four rumblers, two overlords, and four floor eggs are added through
ordinary entity creation; the authored brain and creatures remain. This is a
synthetic renderer stress workload, not a claim about the normal encounter.
AI, physics, particles, and skeletal/material animation remain active.

Compare classic terrain, upgraded 25AE terrain (including supported animation,
UV effects, and material layers), and the additional experimental wetness:

```sh
node tools/quest-bench/run.mjs --scene many-brain-mixed-crowd \
  --lighting on --terrain all --repeats 2 --warmup 10 --seconds 30 --gpu-seconds 10 \
  --output /tmp/quest-many-terrain
```

Two repeats use classic/upgraded/wet/wet/upgraded/classic ordering. All modes
use the same release APK, object assets, object lighting, camera, crowd, and
world-space spotlight. `classic` explicitly sets `no_upgraded_terrain`, overriding the enabled default;
this is not an original-game-versus-remaster comparison of all assets.
`upgraded` has wetness zero; `wet` uses 1.5. Default `--terrain fixture` honors
the fixture without changing existing lighting benchmarks.

Each measured bucket verifies the actual terrain feature/wetness, per-model
mesh counts, lighting, and advancing animations. Static eggs count as meshes
but are explicitly excluded from skeletal animation checks. A nonzero mesh
count proves submission, not visibility: inspect the captured device image too.

For deterministic desktop inspection of the same workload:

```sh
cargo dbgr --benchmark-scene benchmarks/scenes/many-brain-mixed-crowd.json \
  --vr --port 0 --window-size 1200x900
```

Copy the JSON and change `upgraded_terrain` / `terrain_wetness` to capture a
matched comparison. The fixture owns camera placement and its spotlight;
normal tracked-hand lights resume when no fixture is loaded. No saves change.

`--gpu-seconds 10` adds a separate, bounded Adreno counter interval after each
timing/visual sample. Counter IDs are discovered by name from the connected OS;
`gpu-metrics.txt` preserves that mapping. Counters do not run during FPS timing.

The [Quest 120 Hz project](../projects/quest-120hz.md) tracks the 8.33 ms target,
starting with FFR at 90 Hz and a physics-clock correctness gate before 120 Hz.

## Fixed foveated rendering

[Quest 3 16-run results and raw evidence](results/2026-10-09-quest3-ffr.md): App-time savings,
but no demonstrated freshness win; warm-run regression remains under investigation.

Use one release APK at 90 Hz, with upgraded terrain on and wetness off:

```sh
node tools/quest-bench/run.mjs --scene many-brain-mixed-crowd \
  --lighting on --terrain upgraded --ffr all --repeats 4 \
  --warmup 10 --seconds 30 --output /tmp/quest-ffr
```

The four repeats form a balanced Latin square: off/low/high/medium,
low/medium/off/high, medium/high/low/off, high/off/medium/low. Each level
occupies each order position once and every ordered adjacent pair occurs
once. `--ffr fixture` honors the fixture (missing `ffr` means off).
Single levels are also accepted. The fixture overrides `ffr-level.txt`.

Each measured workload record must confirm the requested **and applied**
level; app-PID VrApi telemetry must confirm the same fixed level with no
`D` (dynamic) suffix. Unsupported fallback, absent evidence, or configured
OS foveation overrides invalidate the comparison. Timing and optional
`--gpu-seconds 10` hardware counters are separate intervals.

## CPU and simulation-clock diagnostics

Add `--profile-cpu` to the fixture runner to collect opt-in
`SHOCK2QUEST_CPU_PROFILE` records alongside each one-second timing window.
These report inclusive phase totals/call counts/maxima, scene elapsed time,
actual Rapier step count and accumulated solver time, and a sparse per-frame
CPU wall-time histogram (100 µs bins, bin 500 is overflow at >=50 ms).
Phase timings are nested; do not sum parent and child phases. The CPU interval
runs after `wait_frame` through `end_frame`, including swapchain waits; it is
neither pure CPU execution time nor GPU time. Merge histogram counts across
windows to estimate percentiles; bin upper bounds are conservative, and an
overflow percentile has no finite upper bound.

The diagnostics do not change scheduling, solver dt, refresh rate, or gameplay.
Use scene elapsed versus solver elapsed to audit the existing clock before
raising refresh. A synthetic free-fall audit can be run with:

```sh
cargo test -p shock2vr --lib clock_audit_counts_actual_solver_steps -- --nocapture
```
