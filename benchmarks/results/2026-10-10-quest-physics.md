# Quest evidence for lazy hitboxes

The prototype's next step is worth pursuing: removing Rapier limb maintenance
reduced update time consistently in the measured Many and MedSci workloads.
This is an optimistic estimate, not an implemented lazy-hitbox speedup.

| Mission | Baseline physics | Omit proxies | Reduction | Baseline update | Omit proxies | Reduction |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Many | 1.870 ms | 1.247 ms | 33.3% | 6.540 ms | 5.476 ms | 16.3% |
| MedSci | 1.375 ms | 1.046 ms | 23.9% | 5.164 ms | 4.724 ms | 8.5% |

Quest 3 / Android 14, release aarch64, 90 Hz, 1680×1760 per eye. Each mission was
restarted, warmed for 15 seconds, and sampled for 30 seconds. Two repetitions per
variant used fixed cameras in the authored Many brain chamber and MedSci cryo
room; no actors were added. The player remained at its starting area. Both omission
runs beat both baseline runs for physics and whole-update time in each mission.

Many's baseline physics repeats were 1.864/1.875 ms; MedSci's were 1.323/1.427 ms.
Device temperature rose from 32°C to 38°C, and CPU clocks varied between 2208 and
2361 MHz. Results are workload-dependent; this is not a 120 Hz acceptance test.
A SIMD comparison in the same sweep showed only a 1–2% mean physics difference,
within run variation, so SIMD remains outside this PR.

| Mission / variant | FPS mean / minimum | Stale frames | Torn frames |
| --- | ---: | ---: | ---: |
| Many baseline | 88.02 / 85 | 218 | 19 |
| Many omission | 90.17 / 89 | 80 | 20 |
| MedSci baseline | 88.63 / 84 | 302 | 0 |
| MedSci omission | 89.57 / 87 | 231 | 0 |

Counts cover two 30-second runs; skipped frames were zero. Remaining stale frames
mean the compositor average must not be called stable fresh-frame delivery.

## Provenance and scope

Measurements were collected on research revision `471091bc`, before this focused
PR was extracted onto current main. That revision also contained the earlier
unused-contact-force cleanup and target-write instrumentation; this PR does not
include those changes. These measurements motivate the design, not a performance
claim for the exact PR tip. The baseline APK SHA-256 was
`148907ef217881d476fba8d87134711ba3cc116a79451a191e25354f5fbdda8a`.

[Per-run data](2026-10-10-quest-lazy-hitbox-summary.json) preserves the baseline and
omission timings, nested stages, engine/VrApi distributions, clocks, scene counts,
and target-write counts. The reference path had 488 limb targets/tick in Many and
206 in MedSci; omission had zero. Complete 600-update physics windows crossing
into the measurement interval were excluded. Physics total is prepare + step +
controller + events; nested Rapier stages are not added again.

The omitted path still runs animation, joint transforms, ECS proxies, and scripts.
It does not provide replacement damage bounds or queries, and breaks limb damage
and held-melee contacts. A production implementation must preserve joint
attribution, world occlusion, self filtering, partial coverage/corpse fallback,
and melee behavior before these proxies can be removed. Replacement costs will
reduce the savings measured here.

All six first-pass device captures were visually checked. The initial wall-facing
runs were discarded and all comparisons restarted with fixed cameras. The original
APK hash, mission selector bytes, and removal of temporary selectors were verified
after restoration; Guardian/proximity automation was restored and the app stopped.

On the focused main-based PR branch, the full gameplay library suite with
`damage-query-audit,physics-profiling` passed **2,575 tests, 0 failed, 3 ignored**.
Formatting and diff whitespace checks passed.
