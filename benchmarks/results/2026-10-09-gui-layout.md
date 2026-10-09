# On-demand GUI layout — Quest 3, 2026-10-09

The GUI implementation is independently based on main. Measurements below were
collected on the combined terrain/FFR/fixed-clock benchmark build; the optional
profiling hooks remain in that integration branch. The independent UI, portal,
and FFR changes also pass all 2,563 host gameplay tests together on current main.

Skipping layout for unopened scripted panels reduces Many simulation tick time
by **17.4%** in this A/B/B/A comparison. It does not yet meet the 8.33 ms frame
target. [Raw measurements and provenance](2026-10-09-gui-layout/) retain the
four runs, fixture, CPU profiles, telemetry, and visual verification.

Quest 3, Android 14; release APKs at 90 Hz, 1680 × 1760 per eye, low fixed FFR,
upgraded terrain enabled, wetness off, object lighting on. Both builds use the
opt-in fixed 60 Hz simulation clock. Each run restarts Many's brain mixed crowd
fixture, warms up for 10 seconds, and measures 30 seconds. This is an idle
animated crowd, not combat. APK hashes identify the binaries independently of
the benchmark harness checkout: baseline `ae7fa5a3`, optimized `93afc347`.

| Metric | Baseline A1 | Optimized B1 | Optimized B2 | Baseline A2 |
| --- | ---: | ---: | ---: | ---: |
| Mission update, ms/simulation tick | 6.353 | 5.322 | 5.357 | 6.582 |
| Scripts, ms/tick | 2.348 | 1.244 | 1.228 | 2.453 |
| Physics, ms/tick | 1.811 | 1.828 | 1.862 | 1.848 |
| Visibility, ms/render frame | 1.534 | 1.586 | 1.591 | 1.595 |
| Game update, ms/render frame | 4.826 | 3.784 | 3.870 | 5.102 |
| Mean delivered FPS | 82.83 | 87.23 | 85.87 | 81.03 |
| Minimum reported FPS | 77 | 84 | 83 | 77 |
| Stale frames / 30 seconds | 332 | 142 | 212 | 374 |
| Torn frames | 0 | 0 | 1 | 1 |
| Skipped app frames | 0 | 0 | 0 | 0 |
| VrApi App time, ms | 8.493 | 7.956 | 8.224 | 8.356 |
| Instrumented frame interval mean, ms | 10.610 | 9.634 | 9.866 | 11.090 |
| Frame interval p95 upper bucket edge, ms | 13.9 | 12.6 | 13.0 | 14.4 |
| Mean reported temperature, °C | 34.5 | 35.1 | 39.9 | 39.8 |

Using the mean of each pair, simulation tick time falls from 6.467 to 5.340 ms
(17.4%), delivered FPS rises 5.6%, and stale frames fall 49.9%. Average update
time per rendered frame falls 22.9%; this also reflects fewer simulation ticks
per rendered frame as FPS rises. The per-tick figure is the cleaner measure of
simulation savings. Script time falls 48.5% because unused GUI work disappears,
not because AI runs less often. Active GUI layout is now timed separately under
effects; no panel is open in this fixture.

All four runs pass clock validation and retain six advancing creature animations
and the expected rendered workload. Physics cost remains comparable. Temperature
and automatic device clocks vary, so these are repeated observations rather than
a confidence interval. The slower return-to-baseline run supports the result.
The renderer is unchanged; the small App-time difference is not a demonstrated
GPU optimization. Instrumented frame intervals include synchronization and
waiting, so they are not pure CPU work. Optimized p95 remains above 8.33 ms.

The native A/B attempt is excluded: unrelated compilation resumed after the
initial quiet period. No reliable host speedup is claimed. Earlier unpaced
sampling attributed about 16% of main-thread samples to GUI construction; that
was hotspot attribution, not an elapsed-time benchmark.

Validation: 179 GUI tests, 67 panel-host tests, 3 profiling tests, formatting,
and native/Quest builds passed. Flat and VR normalized UI checkpoints agree
before/after for opening, clicking, looting, closing, reopening, and inventory.
World-panel collider lifecycle also matches. Device captures retain the Many
crowd and upgraded textures. [Before/after media and reproduction scripts](https://gist.github.com/tommy-xr/7fc7b1f8819f0b70860c42ebd5120b1c).

## Next: visibility and physics

Visibility costs about 1.59 ms on every rendered frame. Its entity pass repeats
up to eight BSP lookups per positioned entity, even when its bounds are unchanged;
the existing center-position cache is unused. First investigate caching entity
cell membership while recalculating camera/portal visibility each frame. Cache
invalidation must cover position and dimensions, component removal, entity
destruction, and level replacement. Verify visible entity sets against the
uncached path during movement, teleports, resizing, and portal transitions.

Physics costs about 1.85 ms per simulation tick. The `solver` scope accounts for
1.62–1.65 ms of that, but wraps the entire Rapier pipeline, including collision
detection. Split broad phase, narrow phase, island/constraint work, and CCD before
choosing an optimization. Rapier's internal timers require its `profiler` feature;
measure instrumentation overhead and keep profiling optional. Preserve timestep,
collision behavior, and solver settings. These investigations precede moving the
simulation to a worker; no savings for them have been measured yet.
