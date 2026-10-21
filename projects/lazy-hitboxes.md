# Query-only creature hitboxes

Status: experimental, opt-in Cargo feature `shock2vr/lazy-hitboxes`.

Animated damage volumes can be queried independently of the physics simulation.
The movement capsule remains in Rapier. Limb shapes retain their ECS proxy IDs
and damage-forwarding scripts, but create no Rapier bodies or colliders.

## Query routing

For a ray, first ask Rapier for the nearest world/object hit. Use that distance
as the upper bound for a separate creature query: reject creatures whose padded
union of posed limb AABBs misses the ray, then intersect their cached limb shapes.
Return the nearer result; a Rapier hit wins an exact-distance tie. This reuses
Parry's primitive intersection math without enrolling the limbs in Rapier's
per-tick broad phase or narrow phase.

The shared ray entry point covers projectiles, flat melee, interaction, laser
sights and psi targeting. Membership masks and entity predicates still apply.
Projectile capsule refinement, self filtering, incomplete Overlord coverage and
corpse fallback remain in the existing projectile code. Inside a creature,
exact-distance overlapping limbs use joint index order, rather than Rapier's
spatial-index traversal order. That is a deliberate attribution rule, not exact
legacy attribution parity at coincident surfaces.

Held VR melee compares its existing translation sweep against world geometry
and the independent limb shapes. It retains the nearest blocker and feeds a
limb stop into the existing contact/damage path with the measured closing speed.
A separate posed overlap check produces contact-start edges for enabled held
melee weapons, including a creature moving into a resting weapon. Scripts still
own swing thresholds, damage forwarding and per-creature cooldowns. Inert held
items do not generate limb contacts. Placement and held-item recovery also query
limb occupancy, including the recovery arm corridor.

Animation queues the next pose; it becomes queryable at the next physics step,
matching position-based kinematics. Shapes are cached, and group bounds are
rebuilt from the committed pose. Hidden/unseen limbs and removed entities leave
the query registry. Debug aim points and the existing hitbox wireframe overlay
use the same committed geometry; query-only aim points expose `body_id: null`.

## Verification

Run both backend configurations:

```sh
CARGO_INCREMENTAL=0 cargo test -p shock2vr --lib
CARGO_INCREMENTAL=0 cargo test -p shock2vr --lib --features lazy-hitboxes
CARGO_INCREMENTAL=0 cargo build -p debug_runtime --features shock2vr/lazy-hitboxes
cd tools/shock2-sdk
npm run build
SHOCK2_E2E=1 SHOCK2_EXPECT_LAZY_HITBOXES=1 \
  SHOCK2_RUNTIME_BINARY="$PWD/../../target/debug/debug_runtime" \
  node --test --test-concurrency=1 \
  dist/test/lazy-hitboxes.e2e.test.js \
  dist/test/hitbox-coverage.e2e.test.js \
  dist/test/ranged-hitbox.e2e.test.js \
  dist/test/melee-hitbox.e2e.test.js \
  dist/test/melee-swing-stop.e2e.test.js \
  dist/test/close-range-projectile.e2e.test.js \
  dist/test/vr-melee-contact.e2e.test.js \
  dist/test/grub-ai.e2e.test.js
```

Unit coverage includes conservative bounds, oblique thin extended limbs, direct
shape-query parity, world occlusion, filters, deterministic joint ties, staged
poses, removal, overlap entry/re-entry, placement, and recovery. Existing
projectile fixtures run with Rapier hitboxes in the default build and promote
the same fixture shapes to query-only volumes in the feature build.

The additional backend-specific gameplay test requires aim points with no
Rapier bodies and verifies that those volumes remain hittable after death.
Animated corpses intentionally retain their damage volumes. Existing ranged and
VR melee tests verify actual damage messages and joint attribution.

Verified on the implementation branch:

- Default gameplay library: **2,575 passed, 3 ignored**.
- `lazy-hitboxes` gameplay library: **2,585 passed, 3 ignored**.
- SDK build and all **12** SDK aiming unit tests passed.
- **16** targeted gameplay checks passed across the eight files above, including
  ranged joint attribution, VR swing stopping and once-per-creature billing,
  authored corpse fallback, muzzle/world blocking, grub animation/death, and
  query-only limb coverage. Animated corpses intentionally remain hittable.
- Rust formatting and whitespace checks passed.

## Quest measurement and remaining limits

The [full replacement Quest comparison](../benchmarks/results/2026-10-10-quest-lazy-hitboxes.md)
measured two runs per backend at 90 Hz with low FFR. Many's whole update fell
from **5.161 to 4.552 ms (11.8%)**, and MedSci's from **3.982 to 3.693 ms (7.3%)**.
Physics fell **16.4%** and **14.0%**, respectively. Both replacement runs beat
both baseline runs in each scene. These are fixed idle workloads, not sustained
combat, and do not establish stable 120 Hz.

- Animation evaluation is still eager. Bounds cover a committed snapshot, not
  an entire attack clip or motion envelope. An attached pipe is not automatically
  a damage volume just because the creature holds it.
- Held melee retains the existing translation sweep and endpoint orientation
  checks. This does not add continuous rotational or relative limb-motion CCD.
- The linear scan of creature bounds and per-step rebuild have a cost, included
  in the measured replacement results. Sustained combat with many simultaneous
  queries still needs measurement; omission-only estimates are not delivered gains.
- `damage-query-audit` continues to audit the legacy backend. Its sampling is
  disabled when `lazy-hitboxes` is enabled, since the independent Rapier limb
  reference has been removed. Do not interpret absence of audit output as parity.
- Actual hybrid attack clips/transitions and Quest gameplay validation remain
  necessary before considering a default change. Physics timing is unchanged.
- Independent Claude review found an opt-in melee gap: synthetic overlap contacts
  do not carry limb velocity, and `velocity_at_point` only sees Rapier bodies.
  A moving limb hitting a stationary blade can therefore lose its damage, while
  a retreating limb can overstate closing speed. Preserve committed limb motion
  and add a moving-limb/stationary-blade regression before enabling by default.
- Further opt-in checks: squeeze-to-frob of animated corpses through a query limb;
  capsule fallback when hidden/skipped limbs are pruned but the hitbox marker
  remains; feature-build CI coverage. Default builds retain Rapier limb bodies.

Historical measurements: [query audit](../benchmarks/results/2026-10-10-damage-queries.md)
and [Quest proxy-omission experiment](../benchmarks/results/2026-10-10-quest-physics.md).
