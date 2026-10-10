# Conservative object cell coverage — 2026-10-09

The corner cache optimization preserves an existing visibility hole: a long
object can cross a visible cell even when all corners and its center are in
other cells. The follow-up traverses BSP planes with the interval occupied by
the whole axis-aligned box, then caches that conservative coverage.

This query may admit extra cells. Its correctness requirement is to include
every cell intersected by the supplied bounds, not to produce the smallest set.
It preserves the existing bounds source (`PropPhysDimensions.size` at the
entity position, or center-only fallback without dimensions). Auditing that
source against rotated, offset, scaled, or animated render geometry remains
separate work. Portal traversal and its near-plane shortcuts are unchanged.

## Tests

All **273 dark tests** and **2,565 gameplay tests** pass, with three existing
ignored gameplay tests. New geometric and lifecycle coverage includes:

- A long thin box whose corners **and center** miss the middle cell it crosses.
- Grazing faces, negative sizes, flat/point boxes, invalid leaves, and nonfinite
  positions, with conservative treatment at floating-point boundaries.
- Oblique BSP planes checked against sampled interior points across several
  orientations, thin axes, positions, and aspect ratios.
- Long oblique screen-space portals with every corner off-screen but a visible
  middle, tiny positive overlaps, and zero-area edge contact.
- Camera-visible set changes, empty membership, movement, size changes, HasRefs,
  position removal, and entity destruction.

The dark test target initially failed on an unrelated stale AnimatedModel test
fixture in main. Initializing its required `interaction_triangles` field lets
the existing and new tests compile; no production model behavior changed.

## Visual evidence

Across 24 MedSci flat/VR checkpoints there are **no nonparticle removals**. The
conservative query adds the same railings (239/250), pipes (333/337), window
(464), and security glass (1309) in both presentations. Those draws may remain
occluded; the captures primarily demonstrate parity, not an obvious on-screen
bug reproduction. The geometric regression test directly demonstrates the hole.
Many retains its named benchmark subjects; ordinary spawned creatures and
particles vary between launches.

[PNG/GIF comparisons, identity deltas, and reproduction scripts](https://gist.github.com/tommy-xr/31fa6fe642cc53465d7cc146d584b503).

## Mac performance check

Same unpaced Many workload and six 120-frame batches after 600 warmup frames as
the [cache benchmark](2026-10-09-visibility.md). A is the cached-corner version;
B is the conservative whole-box version. [All samples](2026-10-09-visibility-bounds/).

| Run | Mean wall ms/rendered frame |
| --- | ---: |
| A1 | 3.968 |
| B1 | 3.877 |
| B2 | 4.854 |
| A2 | 4.925 |

The pair means differ by only 1.8% (4.446 → 4.365 ms), while both later runs
slow substantially. **This is inconclusive for performance**, not a claimed
speedup. A planned interleaved comparison was rejected before measurement when
unrelated compilation resumed. No Quest result is available for this follow-up
because the device battery depleted. The original cache's preliminary device
and Mac gains remain separately reported; they must not be attributed to this
correctness change.
