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
node --test tools/quest-bench/run.test.mjs
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

- [Quest 3 object lighting, 2026-09-21](results/2026-09-21-quest3-lighting.md): paired timing runs, GPU counters, and stereo comparisons.
