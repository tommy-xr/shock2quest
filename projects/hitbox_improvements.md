# Hitbox Improvements

Split out from `projects/ragdoll.md`. The physics **ragdoll core is working**
(no explosion, settles, per-bone capsule colliders, joint limits, self-collision,
single-corpse death handoff). What remains is getting the **per-joint collision
shapes (hitboxes) correctly fit and tuned**, and using them consistently for both
the ragdoll and location-based damage.

## Background: how creature hitboxes work

- Creature AI meshes (LGMM `.bin`, `Data/res/mesh/*.BIN`) store vertices in
  **joint-local space** — each joint's verts cluster around that joint's origin
  (confirmed by comparing per-joint vertex-AABB centers ≈ origin to the joints'
  world positions). Skinning = `joint_transform * vert` (the skeleton transforms,
  `Skeleton::get_transforms`/`world_transforms`, are the raw `global_transforms`
  with **no inverse-bind**).
- The Dark engine has **no authored per-joint collision data** (whole-body
  collision is a 1-2 sphere "SphereHat"/OBB; damage is a mesh-polygon raycast →
  segment index). So mesh-derived shapes are the source of truth. (See the
  reference-engine notes in `projects/ragdoll.md`.)

## What's been built

### Shared shape: `dark::hit_box`

`HitBoxShape` (`Cuboid { half_extents, center }` | `Capsule { a, b, radius }`,
joint-local) + `fit_hit_box_shapes(mesh, skeleton)`:
- joint with a **single child** → capsule spanning toward that child (covers the
  bone), radius = max perpendicular distance of the joint's verts to the axis;
- leaf / branching joints → vertex AABB box.

This is the **single source of truth**, consumed by:
- `RagDollManager` (`rag_doll.rs`) — ✅ done (PR #291): builds `SharedShape::capsule`/
  `cuboid` per joint, density for ~uniform mass.
- `hitbox_analyzer` (tool) — ✅ validates fit/coverage/overlap.
- `HitBoxManager` (`hit_boxes.rs`, damage) — ❌ **not yet**; still uses the old
  per-joint AABB (`model.get_hit_boxes()`), so location-based damage volumes are
  still under-covering.

### Tool: `hitbox_analyzer`

`cargo run -p hitbox_analyzer -- <mesh.bin | Data/res/mesh>`. Per creature:
- **bone-segment coverage** (legacy AABB vs fitted) — fraction of each bone
  (parent→child) inside the union of shapes;
- **vertex (surface) coverage** — fraction of skinned verts inside the union
  (sensitive to radius, unlike segment coverage);
- **shape overlap** — fraction of each shape's volume inside a non-adjacent shape;
- per-joint fitted shape dump.

Baseline numbers (GRUNT_P): bone coverage **aabb 59% → fitted 98%**, vertex
coverage **99%**, overall overlap **4%** (worst: LThigh 24%, due partly to the
skeleton hierarchy quirk below). MONKEY/ASSASSIN similar.

## Analyzer verification (all 75 creature meshes, 2026-06-17)

Ran `hitbox_analyzer` across every creature. **Fit/coverage is essentially solved**:
fitted bone coverage **81–100%** (only 3-joint *held weapons* like `WRENCH_H` sit
at ~81%), vertex (surface) coverage **97–100%** everywhere. No coverage work left
on the actual creatures.

**Overlap is the only remaining tuning lever, and it's localized:**
- humanoids (grunts, corpses, players, assassin, …): **2–4%** — already good.
- monkeys / headless / `exphazhh` / `shodan`: 7–14%.
- overlords (`OVERLORD`/`OVERLORO`/`overlote`): **26–31%** — a big root cuboid
  (`j0` half ≈ 1.07×0.95×0.86) that all limb capsules sit inside, *plus* the
  hub-topology artifact (limbs are non-adjacent to the root in the metric).
- robots (`secbot`/`mainbot`/`mpbot`/`prototest`): **37–38%** — bulky boxy bodies
  give fat **max-perpendicular-distance** capsule radii (e.g. `secbot` LToe r=0.90,
  RShoulder r=0.68) packed around a compact torso.
- `test.bin`: **69%** — junk/test mesh; one stray vertex blows a capsule to r=2.21.

Two takeaways the doc didn't have: (a) the **max-distance radius** is the overlap
culprit for robots/degenerate meshes — a high-percentile radius (lever #4) would
target exactly those while leaving humanoids unchanged; (b) much of the
overlord/robot overlap is the **hub-topology metric artifact** (#5), so fix the
analyzer's adjacency before tuning shapes or you'll chase a phantom.

## Open issues / next work

1. **Runtime capsule misplacement — thighs don't connect torso→knee.** In
   `debug_ragdoll --debug-physics` some capsules (notably the thighs) point off-
   axis and don't reach the child body, even though the **bind-pose analyzer shows
   100% coverage**. So the *fit* is right but the *runtime placement* is wrong.

   **Analysis (2026-06-17): the doc's leading "bind-vs-spawn" hypothesis is almost
   certainly NOT the cause.** Working the math: `fit_hit_box_shapes` sets the
   capsule far endpoint `b = inv(bind_world[parent]) · bind_world[child]`, which
   reduces to the child's *local bone offset* `L`. At spawn the body is oriented at
   `R_anim[parent]`, so the capsule points along `R_anim[parent]·b = R_anim[parent]·L
   = child_anim − parent_anim` — i.e. **reach is pose-independent** and `b` need not
   be re-derived. The real suspect is the **conversion**: `add_ragdoll` sets body
   orientation via `get_rotation_from_matrix` (`util.rs`), which shoves the raw
   upper-3×3 into `Matrix3→Quaternion` with **no orthonormalization**. If the joint
   world matrices carry any scale/shear (these meshes use `SCALE_FACTOR`), the
   extracted rotation is wrong *and* the unscaled local `b` is the wrong length →
   capsules point off-axis and fall short. This is the hitbox→ragdoll conversion,
   not the joint/hitbox mapping.

   The new `debug_hitbox` scene (below) renders both placements side by side to
   confirm: **green** = shapes by the full joint matrix (tracks the mesh), **red** =
   shapes by `add_ragdoll`'s decomposed `(position, get_rotation_from_matrix)`.
   Where red diverges from green, the conversion is at fault.
2. **Wire `HitBoxShape` into `HitBoxManager`** (damage hitboxes). `add_kinematic`
   currently takes a box size; needs capsule support. Fixes location-based damage
   coverage too (same root cause).
3. **`debug_hitbox` scene** — ✅ **built.** `cargo dbgr --mission debug_hitbox`
   spawns a creature and overlays, every frame, the fitted shapes placed two ways:
   **green** by the full joint matrix (tracks the mesh) and **red** by the ragdoll's
   decomposed `(position, get_rotation_from_matrix)`. Cycle animation poses with the
   `DebugHitboxCyclePose` input action (HTTP: `POST /v1/input/action`), so it's
   drivable headlessly via the debug runtime. Composes with `--debug-physics` /
   `--debug-skeletons`. The shared wireframe renderer is
   `dark::hit_box::draw_debug_hit_box_shapes`, also wired into
   `dark_viewer --debug-hitboxes` (physics-free, animatable via `--animation`) as
   the fastest fit-only diagnostic. Remaining: act on the red/green divergence to
   fix the conversion (`get_rotation_from_matrix` / scale handling).
4. **Overlap / bounds tuning** — overall overlap is low (4%) but some shapes are
   loose (LThigh 24%; head bounds look big visually). Levers: tighten capsule
   radius (e.g. high-percentile instead of max vertex distance), inset endpoints,
   special-case head/leaf joints. Use vertex-coverage as the guard so tightening
   doesn't drop coverage.
5. **Skeleton hierarchy quirk** — the humanoid skeleton has joint 8 ("Neck") as a
   hub whose children include the thighs (`8:Neck->6:LThigh`), so anatomically
   adjacent parts (thigh/abdomen) count as *non-adjacent* in the overlap metric and
   the parent→child bone direction is unintuitive. Worth understanding before
   over-tuning overlap.

## Status of PRs (as of writing)

Ragdoll stack merged through #284. Hitbox chain (stacked, in review):
`#289 coverage metric` ← `#290 shared fitter` ← `#291 ragdoll capsules`. The
analyzer overlap/vertex-coverage metrics are a further increment on top
(`feat/hitbox-overlap-tune`), to be PR'd.
