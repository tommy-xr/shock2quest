# Conservative damage-query prototype

The `damage-query-audit` feature explores grouping damage shapes by creature,
independently of its physical movement capsule. Gameplay still uses the existing
Rapier proxies, projectile queries, and held-melee contact events. This is a
correctness experiment, not yet a lazy hitbox implementation or a frame-time win.

## Design

Each snapshot unions the world-space AABBs of the creature's actual posed damage
shapes, with outward floating-point slack. A ray missing that union skips the
creature's detailed shape tests. An uncertain/non-finite bound disables rejection.
The detailed query returns the nearest limb and accepts a maximum distance so a
caller can clip against a nearer world blocker. Unknown owners get separate
groups rather than an assumed actor capsule.

A limb extending outside the movement capsule therefore still expands the damage
bound. The bound covers the snapshot's current pose; it is **not** a swept bound
or an envelope proven to cover every animation. It preserves existing damage
shapes, not necessarily every visible vertex or an attached weapon's geometry.

Every 600 physics updates, the opt-in audit snapshots live hitbox colliders after
Rapier updates its query structures and compares four rays per shape (three axis
probes and one starting inside) against Rapier's hitbox query. Gameplay never uses
the experimental answer. Equal-distance hits on different proxies are reported
separately because overlapping joints can disagree on damage attribution.

## Validation

Geometry tests cover extended limbs, a synthetic thin pipe-length shape rotated
through 121 poses, grazing/inside rays, false-positive bounds, nearest hits,
world-distance clipping, uncertain/overflowing bounds, and 4,000 seeded oblique
rays compared with exhaustive shape queries. The synthetic swing is not a replay
of the actual hybrid attack animation.

Full gameplay library validation with the audit feature: **2,567 passed, 0 failed,
3 ignored**, including eight new geometry tests. `cargo fmt --all -- --check`
and `git diff --check` also passed.

## Reproduction

```sh
CARGO_INCREMENTAL=0 cargo test -p shock2vr --lib --features damage-query-audit
CARGO_INCREMENTAL=0 cargo build -p debug_runtime --features shock2vr/damage-query-audit
node benchmarks/results/2026-10-10-damage-queries/run.mjs
```

The SDK script uses stationary default-spawn VR sessions in Many and MedSci, each
stepped for 1,800 fixed-timestep frames. It shuts down each owned runtime. Paths
are resolved relative to this repository; the SDK's built `dist` is required.

## Live audit results

| Mission | Creatures / shapes | Probe rays | Hit/distance mismatches | Equal-distance proxy disagreements |
| --- | ---: | ---: | ---: | ---: |
| Many | 34 / 488 | 5,856 | 0 | 164 |
| MedSci | 14 / 206 | 2,472 | 0 | 83 |

All six sampled snapshots completed without skipped probes. A distance match uses
`tolerance = 1e-4 * (1 + abs(reference_toi))`; it is not a bitwise equivalence
claim. Overlapping joints can return different proxies at matched distances,
particularly with solid rays starting inside shapes. Those 247 disagreements
remain an attribution compatibility issue, not proof of interchangeable results.

The bounds rejected about 96.3% (Many) and 92.2% (MedSci) of exhaustive detailed
shape tests. **This is relative to brute force, not Rapier**, which already has a
spatial index. In these diagnostic batches the linear group prototype took
4.44–4.70 ms for 1,952 Many rays versus Rapier's 1.85–1.94 ms; MedSci took
2.05–2.12 ms for 824 rays versus 1.03–1.14 ms. Snapshot construction took
18–48 microseconds. Compilation ran concurrently, so these are not controlled
performance benchmarks and do not establish a shipping frame-time improvement.

The expected opportunity is eliminating hundreds of moving limb proxies from
Rapier's per-tick maintenance. That benefit is not measured by this audit, which
retains them all. Query routing and a spatial index over creature bounds should
be evaluated together with removal costs and held-melee replacement.

Raw measurements are in `2026-10-10-damage-queries/results.json`.

## Before removing proxies

- Replace per-joint Rapier maintenance with immutable damage-shape data and one
  conservative bound per creature; measure snapshot construction as well as
  Rapier broad/narrow-phase savings. The current audit still pays all proxy costs.
- Preserve projectile self filtering, nearest world occlusion, joint attribution,
  partial hitbox coverage (including Overlord fallback), and corpses with no live
  hitboxes. The standalone max-distance test is not an end-to-end projectile test.
- Supply held-melee overlap/contact or swept queries before removing the proxies
  it currently depends on; preserve per-creature damage cooldowns.
- Replay actual attack clips and transitions, including the hybrid pipe swing.
  For moving melee shapes, endpoint AABB unions alone cannot cover arbitrary
  intermediate rotation; require a conservative motion bound and sweep tests.
- If skipping pose evaluation too, use a separately validated animation envelope
  or retain eager pose evaluation. A stale current-pose bound is unsafe.

The next useful performance experiment is projectile-only routing with the old
path retained as a reference, followed by an explicit replacement for held melee.
Worker-thread simulation remains a separate, larger change.
