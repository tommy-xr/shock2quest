# Visibility cell cache — 2026-10-09

Caching unchanged entity-to-cell lookups reduced Quest visibility preparation
from **1.562 ms to 0.682–0.690 ms** in Many (about **56%**). The final return-to-
baseline run is excluded: the headset lost focus and disconnected when its
battery depleted. These are preliminary device results, not a completed A/B/B/A
or evidence of stable 120 Hz.

The implementation is independent of terrain, UI, FFR, and fixed-clock PRs.
Measurements use the combined benchmark build: Quest 3 release APK, 90 Hz,
1680 × 1760 per eye, low fixed FFR, upgraded terrain, wetness off, lighting on,
fixed 60 Hz simulation, Many brain mixed crowd. Each run warms up 10 seconds
and measures 30 seconds. [Raw results and exact binary hashes](2026-10-09-visibility/).

| Quest metric | Baseline A1 | Cache B1 | Cache B2 |
| --- | ---: | ---: | ---: |
| Visibility, ms/render frame | 1.562 | 0.690 | 0.682 |
| Portal traversal, ms/frame | not split | 0.259 | 0.257 |
| Entity classification, ms/frame | not split | 0.425 | 0.418 |
| Physics, ms/simulation tick | 1.833 | 1.880 | 1.856 |
| Mean delivered FPS | 86.03 | 87.07 | 87.73 |

Portal traversal is a smaller remaining cost than object classification in this
scene. The CPU saving is not an equivalent FPS increase: rendering and simulation
still consume the rest of the budget. Per-run minimum FPS, stale/torn frames,
App time, temperatures, and frame histograms are retained in `metrics.json` and
the raw results. No GPU-counter run was performed for this change.

## Mac proxy

Apple M3, 16 GiB, optimized development binaries with debug assertions. Four
independent launches in A/B/B/A order; each warms up 600 fixed simulation frames
and measures six batches of 120 rendered frames. The hidden runtime is unpaced.
Compiler activity is checked before each launch and after every batch.

| Run | Mean wall ms/rendered frame |
| --- | ---: |
| A1 | 4.177 |
| B1 | 3.928 |
| B2 | 3.892 |
| A2 | 4.438 |

The mean of each pair is **4.308 → 3.910 ms, a 9.2% reduction**. This includes
update, scene preparation, visibility, GPU drawing, swap, and amortized HTTP
overhead; it is not a visibility-only timer or headset FPS prediction. A2's first
three batches were slower; report all samples rather than selecting its faster
half. Ordinary AI/particle variability also remains. Binary hashes, samples, and
the SDK driver are retained for reproduction.

## Correctness and limits

Three focused tests cover cache invalidation, empty lookups, camera-visible set
changes, teleports, resizing, position removal, destruction, and HasRefs changes.
The independent UI/portal/FFR changes together pass all 2,563 gameplay tests on
current main; the Android release build and formatting pass.

Nonparticle rendered identities match all 24 MedSci flat/VR checkpoints and six
repeat Many checkpoints. The initial Many baseline included another Midwife; a
second baseline omitted it too, and recorded positions show world-state variation.
Particles vary between runs. No missing structural surfaces were observed.
[Still images, looping checkpoint GIFs, identity comparisons, and reproduction](https://gist.github.com/tommy-xr/25a9cdbe7a947adc9b7b15de3618319d).

This first optimization preserves the existing corner-based cell test and its
limitations. A separate correctness change must cover long objects crossing a
visible cell with no corner inside; checking only the midpoint is insufficient
as well. Broader audit targets include oblique and near-plane portals, the first-
two-level clipping bypass, multiple entrances, and cyclic portal graphs. Test
these geometrically rather than treating agreement with the old algorithm as
proof of correctness.
