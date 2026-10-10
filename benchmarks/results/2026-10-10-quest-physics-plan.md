# Lazy-hitbox cost experiment

The `physics-profiling` feature enables aggregate Rapier stage timings. Normal
builds contain neither the instrumentation nor the omission switch.

A file at `<data-root>/hitbox-benchmark.txt` containing `omit-proxies` makes newly
created HitBoxManagers omit Rapier limb bodies and target writes. Animation,
joint transforms, ECS proxy entities/components, and scripts continue running.
The startup marker reports `omit_proxies=true gameplay_valid=false` explicitly.

**This mode is not playable:** limb damage and held-melee contacts are incomplete.
It estimates costs available to remove; conservative replacement bounds, queries,
and melee handling are not charged. Remove the file and restart to restore normal
behavior. Do not enable `damage-query-audit` while measuring costs, because its
synthetic query batches add work.

For Quest measurements, use release APKs with `shock2vr/physics-profiling`, back
up the installed APK and selectors, hold the mission/camera/refresh rate constant,
warm up 15 seconds, and sample 30 seconds. Repeat baseline and omission in reverse
order. Require focused XR, inspect device captures, and compare full update time
alongside the nested Rapier stages. Restore the original APK, selectors, and
proximity automation afterward.

See [the measured Quest results](2026-10-10-quest-physics.md) for the original
experiment, its source revision, and limitations. SIMD is not part of this PR.
