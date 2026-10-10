# Quest physics comparison protocol

Compare Rapier SIMD and the potential savings from omitting moving limb proxies
on the same Quest 3, using release APKs with `physics-profiling`. The damage-query
audit feature must be off so its synthetic ray batches do not pollute timings.

## Variants

- **Scalar:** `shock2vr/physics-profiling`, no marker file.
- **SIMD:** same source plus `shock2vr/physics-simd`, no marker file.
- **Omit proxies:** scalar APK with `/sdcard/shock2quest/hitbox-benchmark.txt`
  containing exactly `omit-proxies` (surrounding whitespace allowed).

The marker is read once when constructing a HitBoxManager and is compiled only
with `physics-profiling`. A log marker explicitly reports `omit_proxies` and
`gameplay_valid`. Omission retains animation, joint transform calculations,
ECS proxy entities/components, and scripts. It omits Rapier limb bodies and
kinematic target writes. **This deliberately breaks limb damage and held-melee
contacts. It measures an optimistic cost-removal experiment, not a playable
lazy-hitbox implementation.** No conservative replacement bounds or replacement
queries are charged to this variant. Do not distribute this configuration.

## Sequence and acceptance

1. Back up the installed APK and the exact contents/existence of mission,
   benchmark-scene, debug-port, and hitbox-benchmark selector files.
2. Use default-spawn Many and MedSci, with no scene fixture, to match the Mac
   audit. These are not the crowded Many brain fixture. Keep the headset still
   and charging; preserve identical view size, refresh rate, and render settings.
3. Run scalar, omit, SIMD, SIMD, omit, scalar. For each variant, restart each
   mission, require focused rendering, warm up 15 seconds, and sample 30 seconds.
   Save the battery state before/after, runtime logs, telemetry, and first-pass
   device captures. Reject missing/failing telemetry or changed refresh rates.
4. Check the marker matches the requested mode, and compare kinematic target
   counts to establish the manipulation actually removed limb maintenance.
5. Compare nested Rapier stages and total physics wrapper time separately from
   whole update time and compositor App time. Report minimum delivered FPS,
   stale/torn frames, and skipped frames beside means. Do not add nested stage
   counters to wrapper totals. Exclude startup/warmup physics windows.
6. Compare both repetitions individually before averaging; substantial drift or
   workload differences require repeats rather than a clean percentage claim.
7. Restore the original APK, selectors, and Guardian/proximity automation even
   on a failed run. Verify restoration and stop the test app.

Build in `runtimes/oculus_runtime` using its documented Android SDK setup:

```sh
cargo apk build --release --features shock2vr/physics-profiling
# Preserve the emitted APK before building the next variant.
cargo apk build --release --features shock2vr/physics-profiling,shock2vr/physics-simd
```

The existing `quest-benchmark.mjs` collects the device metrics:

```sh
node .claude/skills/oculus-profiling/scripts/quest-benchmark.mjs \
  --mission many.mis --warmup 15 --seconds 30 --output /tmp/quest-physics-run
```

## Status

Device preflight succeeded (Quest 3, charging, battery 97%, remaster KPF present).
The installed APK and selectors were backed up. Wi-Fi ADB then disconnected
before any installation or configuration changes. Device measurements are pending;
there is no measured Quest speedup yet.

Both full aarch64 Android release APK builds succeeded (scalar and SIMD).
Artifact SHA-256:

- Scalar: `148907ef217881d476fba8d87134711ba3cc116a79451a191e25354f5fbdda8a`
- SIMD: `d3a4711a53bf6d054a491fbe41c3afd74ec00b53141f6cedb2328965facb4b4c`

The benchmark parser suite passed all 9 tests. Formatting and diff whitespace
checks passed. No APK was installed, no marker files were changed, and no
Guardian/proximity automation was enabled during this disconnected attempt.
The full gameplay library suite with `physics-profiling,physics-simd` passed:
**2,568 passed, 0 failed, 3 ignored**. This validates the default (proxies enabled)
path; it does not make the deliberately incomplete omission mode gameplay-safe.
