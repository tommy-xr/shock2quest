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

## Open issues / next work

1. **Runtime capsule misplacement — thighs don't connect torso→knee.** In
   `debug_ragdoll --debug-physics` some capsules (notably the thighs) point off-
   axis and don't reach the child body, even though the **bind-pose analyzer shows
   100% coverage**. So the *fit* is right but the *runtime placement* is wrong —
   the all-bind analyzer can't see it. Hypotheses to chase:
   - **Bind-vs-spawn frame mismatch:** the fitted capsule endpoint `b` is computed
     in the bind skeleton frame (`fit_hit_box_shapes` uses
     `skeleton.world_transforms()`), but ragdoll bodies spawn in the *death/animated*
     pose (`RuntimePropJointTransforms`). Candidate fix: re-derive the capsule far
     endpoint in the ragdoll from the actual spawn transforms (single-child offset
     in the joint's current-local frame), keeping the radius from the shape. (A
     partial attempt was started and reverted to keep the branch clean.)
   - **Joint misconfigured in hitbox→ragdoll conversion** (user hypothesis): verify
     the per-joint body orientation / collider frame used when converting the shape
     to a Rapier collider matches the joint frame the mesh skins with.
   - Add a non-bind check to the analyzer (or a `debug_hitboxes` scene, below) so
     this class of bug is catchable offline.
2. **Wire `HitBoxShape` into `HitBoxManager`** (damage hitboxes). `add_kinematic`
   currently takes a box size; needs capsule support. Fixes location-based damage
   coverage too (same root cause).
3. **`debug_hitboxes` scene** — spawn a creature and render its colliders in a few
   poses (bind + a couple animated) to eyeball fit and catch the runtime-placement
   class of bug. (User-requested.)
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
