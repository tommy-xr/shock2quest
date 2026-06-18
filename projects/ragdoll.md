
# Ragdoll Deaths Implementation Plan

## Goals

- Replace the current canned death animation with physics-driven ragdolls for humanoid creatures.
- Spawn a dedicated ragdoll entity on death that inherits the deceased model pose, then simulate it using Rapier rigid bodies and joints.
- Keep visual skinning and hit detection in sync with the simulated skeleton so the corpse interacts believably with the world.

## Update (2026-06-18): the rig settles — rapier 0.31 + compliant joints

The 2026-06-15 work below got the rig *built*, but it never came to rest: it
perpetually jittered (`max_linear_speed` stuck at 3–7 even after landing). Root
cause and fixes, all verified headlessly via the debug runtime:

- **Not detachment or self-collision — the joints injected energy.** The new
  `GET /v1/physics/joints` endpoint (per-joint anchor separation + applied
  impulse, labeled by bone) showed the impulse joints holding a permanent
  unsatisfiable residual at "rest". rapier's near-rigid default joint stiffness
  (`SpringCoefficients.natural_frequency` defaults to 1e6) rigidly fights that
  residual every step on the hub-and-spoke skeleton and pumps energy.
- **Fix: upgrade rapier 0.19 → 0.31 + compliant joints.** 0.31 brings the v0.29
  velocity-solver rework and v0.31 per-joint `softness`. Compliant joints
  (`SpringCoefficients { natural_frequency: 60, damping_ratio: 2.0 }`) absorb the
  residual instead of fighting it — the rig now collapses into a heap and rests.
  (0.31 is the last nalgebra-based release; 0.32+ migrates the math to glam, a
  much larger change, so we pinned 0.31. Note: higher `damping_ratio` = *more*
  compliant per rapier's API.)
- **Dedupe joints.** The Dark skeleton lists some joints twice (a torso's main
  joint is also a fixed point on the parent torso; see `ss2_skeleton::create`),
  building a *duplicate* impulse joint between the same body pair (pelvis hub 8 ↔
  abdomen 18) and over-constraining the hub. Now exactly one joint per child body.
- **Degenerate colliders crashed the new broad-phase.** rapier 0.31's BVH
  broad-phase panics (`parry bvh_binned_build`, "index out of bounds") on a
  collider with a non-finite/zero AABB. We were creating such colliders for some
  objects (zero-size triggers; earth.mis entity 7 had an *infinite* dimension) —
  `add_kinematic`'s `size >= 0.0` assert passed both 0 and ∞. The old SAP
  broad-phase tolerated them; the BVH one does not. **Fix:**
  `sanitize_collider_size` clamps every cuboid collider's size to a finite,
  positive, bounded range at the `add_dynamic`/`add_kinematic` boundary.
- **Deterministic stepping.** The debug runtime now steps at a fixed 60 Hz, so
  `{"frames":N}` advances a real, predictable amount of sim time (it used to use
  wall-clock dt, which made ragdoll-settle measurements erratic).

### Known remaining issue: the hips sag (~4 cm)

`GET /v1/physics/joints` localizes it: only the two hip joints (bone 8 → 6/7)
hold ~4 cm separation while every other joint closes to <0.5 cm — the heavy
thighs stretch the compliant joint under gravity. Stiffness tuning is
non-monotonic (stiffer re-introduces energy). Candidate fixes (untried / partly
tried): per-joint stiffer hips/shoulders; child-origin joint anchoring (worse on
0.19, may behave on 0.31's solver); or giving the tiny pelvis-hub body real
inertia.

### New diagnostic tooling (this effort)

- `GET /v1/physics/joints` — per-impulse-joint anchor separation + impulse, bone-labeled.
- `debug_hitbox` scene + `dark_viewer --debug-hitboxes` — overlay fitted hitbox
  shapes (green) vs the ragdoll's decomposed collider placement (red) to separate
  a fit/mapping bug from a conversion bug; the `DebugHitboxCyclePose` input action
  cycles poses (HTTP-drivable). The construction/fit was verified correct; the
  remaining issues are dynamics, not placement.

## Status & Investigation Log (2026-06-15)

The rig had been wired end-to-end (Parts 3–5 below marked "✅") but was not
convincing because it was **broken, not merely untuned**. This section records
what was wrong, how we found it, and how each fix was verified. The "✅" markers
on the historical plan further down are left as-is for context.

### How we test (the iteration loop)

All diagnosis was done **headlessly**, without watching a VR/desktop window, using
the debug-runtime physics introspection added for this effort:

1. `cargo dbgr --mission debug_ragdoll --port N` (optionally `--debug-physics` to
   draw colliders/joints). NOTE: no extra `--` after `cargo dbgr`.
2. Frame-step in small deterministic batches (`POST /v1/step {"frames":10}`); the
   scene kills the pipe hybrid and spawns the ragdoll at a fixed frame count.
3. `GET /v1/ragdoll/metrics` (added in PR #282) for the purpose-built per-corpse
   numbers: `max_linear_speed`, `max_angular_speed`, `min_y` (floor penetration),
   `max_nonadjacent_overlap` (interpenetration between non-adjacent limb AABBs),
   and `max_drift`. (`GET /v1/physics/bodies?entity_id=<corpse>` is the lower-level
   fallback for raw per-body position/velocity.)
4. Interpretation: a healthy ragdoll **settles** (speeds → ~0, y near the floor)
   with low `max_nonadjacent_overlap`; a broken one either **explodes** (huge speed,
   y → large negative), **diverges** (speed *grows* over time = solver instability /
   energy injection), or **interpenetrates** (high overlap).
5. `POST /v1/screenshot` for a visual sanity check (camera matches desktop).

### Findings, fixes, and verification (in order)

1. **Spawn-time explosion — self-collision between jointed bodies.** Bodies used
   `CollisionGroup::selectable()`, whose filter `ALL_COLLIDABLE` includes
   `SELECTABLE`, so adjacent overlapping joint bodies ejected each other every
   frame. **Fix:** new `CollisionGroup::ragdoll()` (members `SELECTABLE`, filter
   `WORLD` only) — limbs hit the floor but never each other. **Verified:** max
   speed 133 → ~1.8, y −904 → resting on floor. (PR #277)

2. **Limbs were point-balls (flat puddle).** `model.get_hit_boxes()` was never
   used. **Fix:** size each limb collider as a cuboid from the joint's hitbox
   AABB, offset to the box center in joint-local space (body origin stays at the
   joint so skinning is unaffected). **Verified:** limb-shaped boxes visible under
   `--debug-physics`. (PR #278)

3. **New instability after (2): the rig flailed and limbs spun forever and never
   settled.** Root cause = **mass ratio**, not collision or joint limits. Box
   sizing produced masses from ~0.0009 (extremity) to ~0.33 (torso), a **~365:1**
   ratio, and Rapier's impulse-joint solver injects energy with large connected-
   body mass ratios. **Diagnosis:** measured angular speed *climbing past 88 rad/s
   and diverging*; reproduced with joint limits removed (so limits were not the
   cause) and with damping-only (still diverged). **Fix:** derive per-collider
   density for a **uniform target mass** (~1:1 ratios; inertia still scales with
   shape) + linear/angular **damping**. **Verified:** angular speed now *decreases*
   and fully settles (max angular → ~0.0, max linear ~0.3). (PR #278)

#### Hypotheses ruled out

- **Ragdoll colliding with the unremoved creature capsule / its AI hitboxes:** no.
  The ragdoll group filters to `WORLD` only; the creature capsule is `ENTITY` and
  its hitboxes are `HITBOX`, so the bidirectional group check fails both ways.
- **Joint limits causing the spin-up:** no — divergence reproduced with limits
  removed. (Naive limits *do* cause a separate problem, see below.)
- **Rapier version:** did not upgrade (0.19 → 0.33 is a large breaking jump; a
  newer solver might mask but wouldn't fix an extreme-mass-ratio config).

### Remaining work (subsequent PRs on the stack)

1. ✅ **Per-bone joint limits** (PR #280). Cone limits per joint from
   `HUMANOID_JOINT_LIMITS` in `creature/creature_definitions.rs`, measured from
   rest-pose local frames. (Knees/elbows are still cones, not true hinges — see
   realism backlog.)
2. **Seamless death→ragdoll handoff.**
   - ✅ **Step 1 (PR #281):** spawn the ragdoll as a dedicated corpse entity from
     the creature's *current* bone world transforms (no offset) and remove the
     original creature, so the corpse replaces it in place.
   - **Step 2 (next): swap at the moment of death with seeded velocity** — seed the
     corpse bodies with the creature's linear/angular velocity (and ideally an
     impulse from the killing blow) so it reacts to how it died instead of
     collapsing straight down.
   - **On deck: blend** animation→physics over a few frames (per-bone weight blend)
     for the smoothest result; only if the hard swap looks jarring. Also: wire the
     handoff to *real* death, not just the debug scene.

### Realism & verification backlog

These came out of reviewing the settled corpse: it's much better than the puddle
but not yet passable, and we lacked objective checks for "is this right". The
verification harness and the headline realism fix are now done; the rest remain.

1. ✅ **Verification harness (PR #282).** `GET /v1/ragdoll/metrics` reports
   `max_linear_speed`, `max_angular_speed`, `min_y`, `max_nonadjacent_overlap`
   (interpenetration between non-adjacent limb AABBs; jointed pairs excluded), and
   `max_drift`. Built on `PhysicsWorld::body_velocities`/`body_world_aabb` and
   per-ragdoll adjacency/spawn-position tracking. This is what makes the items
   below objectively verifiable.
2. ✅ **Self-collision with adjacent-pair exclusion (PR #283) — the main realism
   blocker.** Self-collision had been *fully off* (`CollisionGroup::ragdoll()`
   filtered to `WORLD` only), so torso/legs/head interpenetrated. Fix: filter now
   `WORLD | SELECTABLE` (limbs collide with each other) plus
   `contacts_enabled(false)` on each joint so *directly jointed* pairs don't
   re-explode. **Verified:** `max_nonadjacent_overlap` 0.44 → 0.24 while staying
   settled (max linear ~0.03, max angular ~0.34, no floor penetration).
3. **Hitbox-fit validation across the model corpus — in progress.**
   - ✅ **Reference check (background agent):** the Dark engine has *no* authored
     per-joint collision data. Whole-body collision is a 1–2 sphere "SphereHat" /
     OBB (`sPhysDimsProp`, whole creature only); damage hit-location is a raycast
     against animated mesh polygons → segment index. The LGMM `mms_segment.bbox`
     field is never populated; `.cal` skeletons store joint positions + bone
     lengths/directions but **no limb thickness**. Conclusion: mesh-derived shapes
     are the source of truth — the recommended upgrade is **oriented (bone-aligned)
     boxes/capsules** instead of axis-aligned (AABBs inflate for diagonal limbs),
     keeping `HUMANOID_HIT_BOXES` as the semantic (damage-role) layer.
   - ✅ **`hitbox_analyzer` tool (PR #285, off main):** `cargo run -p
     hitbox_analyzer -- <mesh|dir>`. Added
     `SystemShock2AIMesh::joint_vertex_positions()` to `dark`. v2 loads each mesh's
     `.cal` skeleton and reports, per joint: current model-space AABB, recommended
     **joint-local** AABB + center, inflation (model/local vol), and a recommended
     capsule (axis/radius/half-height).
   - ⚠️ **v2 inflation ≈ 1.00× across all creatures** (joint-local dims = model
     dims with axes permuted; SS2 bind rotations are axis-aligned ~90° swaps), so a
     bone-aligned box is never smaller than the AABB. Oriented colliders give no
     win.
   - ❌ **Joint-local collider sizing was tried and REVERTED — the model-space
     boxes are already correct.** The hypothesis was that `rag_doll.rs` sizes
     colliders from model-space dims/center but attaches them in the joint frame
     (mis-oriented). Implementing it (transform the hitbox AABB by the inverse bind
     world transform) **regressed**: the ragdoll no longer settled (max angular
     ~17 vs ~0.3) and overlap was unchanged (~0.27 vs ~0.24). Reason: the bodies
     are placed at the same per-joint transforms the mesh skins with
     (`Skeleton::get_transforms`/`world_transforms` are both the raw
     `global_transforms` — **no inverse-bind**), so the hitbox vertices are already
     effectively in the collider/body frame. Applying the inverse bind a second
     time double-transforms the box (the tool's large joint-local "centers", e.g.
     toe 1.40, were the tell). **Conclusion:** the current model-space sizing is
     correct; the analyzer's `model dim` column is the meaningful collider size,
     and its `local dim`/inflation/capsule columns are based on a wrong frame
     assumption and should be ignored (a follow-up could drop them).
   - ➡️ The residual ~0.24 `max_nonadjacent_overlap` is therefore **genuine limb
     overlap** of a correctly-sized rig when crumpled, not a sizing bug. No
     collider-resize work is warranted; further realism, if pursued, comes from the
     death-handoff (velocity-seeded) and joint-tuning items, not box sizing.
4. **Handoff pose-continuity / skinning convention.** Physics placement at handoff
   is exact *by construction* (corpse seeded from the same `world_joint_transforms`
   the renderer skins with; `max_drift` ≈ 0 right after spawn confirms no snap). The
   open piece is the **physics→skinning transform convention** — does the mesh skin
   to the same place the physics bodies are (raw joint transforms vs.
   global×inverse-bind)? Needs a render-side check, not a physics metric.
5. **True hinge joints** for knees/elbows (single-axis limits) instead of cones —
   needs the per-bone hinge axis, which the fit/skeleton analysis (#3) can help
   determine.
6. **Wire debug-scene input over HTTP (harness gap).** `DebuggableScene::set_input`
   is stubbed for `MissionCore`, so the debug runtime can't drive contextual hand
   input (trigger/grab/drop) or the `debug_ragdoll` force controls (left-trigger
   impulse, right-squeeze pull) over HTTP — they only work interactively on
   desktop. Wiring this would let an agent exercise force-on-ragdoll tests and hand
   interactions headlessly (e.g. verify impulse → expected velocity via
   `/v1/ragdoll/metrics`), closing a gap in the iteration loop. Note the debug
   runtime owns the `InputContext` at the loop level, so the fix likely lives there
   rather than in `MissionCore`.

**Tooling unblocked (PR #276, merged):** `/v1/physics/bodies` enumerates the raw
Rapier `RigidBodySet` (was a stub), `?entity_id=N` scopes to one ragdoll, debug
scenes are introspectable via `GameScene::as_debuggable()`, and `debug_ragdoll`
spawns the ragdoll on a deterministic frame counter with a fixed camera matching
the desktop default view.

## Files To Reference
- shock2vr/src/physics/mod.rs
- dark/src/ss2_skeleton.rs
- shock2vr/src/creature/hit_boxes.rs
- dark/src/model.rs
- shock2vr/src/scenes/debug_ragdoll.rs

## Previous Implementation

A previous attempt was tried in ragdoll-failed-attempt-1.md, and some learnings are collected there. Because of the challenges, I'm proposing a new incremental plan.

## Revised Implementation

The revised implementation has two improvements over the prior implementation:
1. More incremental changes - we can more easily test implementation build-over-build
2. Simplification - by avoiding the animation player / existing Model, and creating a simpler RagDoll that directly sets skinning data, we can avoid a lot of the problematic coordinate system transforms from the prior approach.

## Implementation Plan

### Part 1: Add visualization for rigid body joint contraints

1. Add a debug_joint_constraint test scene. This scene should add 3 cuboid rigid bodies that are attached by impulse joints, and allow for applying forces or impulses. 
1. Add visualization to the DebugRenderer in debug_render_pipeline DebugRenderBackend - add a visualizer for DebugRenderObject::ImpulseJoint

Deliverable: We can run `--mission=debug_joint_constraint`, see the joints and rigid bodies bound with the joint, and apply forces to verify that the joint works. This validates that we fully understand how to create forces in the engine and that piece is working correctly.

__Status__: ✅ Implemented. The new debug mission (`shock2vr/src/scenes/debug_joint_constraint.rs`) spawns a five-link impulse-jointed chain, exposes force/impulse controls, and the physics debug renderer now colors `DebugRenderObject::ImpulseJoint` lines so the constraints are clearly visible (`shock2vr/src/physics/debug_render_pipeline.rs`).

## Part 2: Add debug visualization for skeletons
1. For desktop_runtime, add a `--debug-skeletons` command.
2. In `ss2_skeleton.rs`, add `debug_draw(&self, global_transforms: &[Matrix4]) -> Vec<SceneObject>`. The `global_transforms` slice is the same per-joint world matrix array we already compute when evaluating the skeleton each frame, so this helper just consumes that existing data to emit debug geometry.
    a. Iterate across `self.bones`, which already contains the joint IDs and parent relationships. For each bone, place a sphere at `global_transforms[bone.joint_id]` and draw a line to `global_transforms[bone.parent_id]` to visualize the hierarchy.
3. Add a function to `model.rs` that is `draw_debug_skeleton()` - for an animated model, this will call into the skeleton helper above and push the returned scene objects into the mission debug renderer.
4. When `--debug-skeletons` is active, draw all active skeletons in `mission_core` for each model
5. Also when `--debug-skeletons` is active, fade animated model meshes and disable their depth writes so the debug skeletons remain visible through the geometry.

Deliverable: We can run an existing mission (like debug_ragdoll) with `debug-skeletons` and see the skeleton and parent-child relationship visualized. This ensures we understand the world-space transforms and the parent-child relationship of the hierarchy.

__Status__: ✅ Implemented. Desktop/debug runtimes accept `--debug-skeletons`, `Ss2Skeleton::debug_draw` + `Model::draw_debug_skeleton` build the bone spheres and link lines, `MissionCore::render` injects them, and skinned meshes are automatically faded (with depth writes disabled) so the skeleton overlay is easy to see.

## Part 3: Clone the skeleton

The goal for this deliverable is, when we run the `debug_ragdoll` scene, once the applydamage effect is done, we _clone_ the skeleton into a static ragdoll 1 unit above the current item. This will allow us to visualize that the layout is correct.

To do this, we'll create a new `ragdoll_manager` and `ragdoll` struct. We'll create this in shock2vr/src/creature/rag_doll.rs - the closet parallel will be the creature/hit_boxes.rs module.

The `RagDollManager` will be instantiated and owned by `mission_core`, and will be responsible for creating the ragdolls. It will store a `HashMap` of `<EntityId, RagDoll>`. 

`RagDollInfo`
- `bones: Vec<Bone>` - an array of bones, to understand the parent/child relationships
- `initial_global_transforms: Vec<Matrix4>` - the initial global transforms 

`RagDoll` - state kept by rag doll manager
- `bones: Vec<Bone>` - an array of bones, to understand the parent/child relationships
- `initial_global_transforms: Vec<Matrix4>` - the initial global transforms 
- `physics_entities` - the list of Rapier handles (rigid bodies + joints) that were created as part of the ragdoll.
- `physics_entity_to_bone: HashMap<JointId, RigidBodyHandle>` - a dictionary that tracks the rigid body handle that should correspond to each bone transform. 
- `bone_frame_offsets: HashMap<JointId, Matrix4>` - captures the transform between the rigid body frame (often centered within a hitbox) and the actual bone/joint origin used for skinning so we can translate rigid body poses back to joint poses.
- `latest_global_transforms: Vec<Matrix4>` - the latest global transforms, which are synced from the physics entities. Initially, this will just be taken from initial_global_transforms.
- `scene_objects: Vec<SceneObject>` - cloned renderables from the original model so the manager can push them to the renderer without keeping an `AnimationPlayer`.

`RagDollManager`
- `new` -> create an empty instance
- `update` -> update the rag doll manager. For each managed ragdoll, we'll synchronize the _global_ (world) positions. **This will be implemented in phase 4**
- `add_ragdoll` -> given an entity, model, and physics world, this will add a ragdoll. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints. The flow will be as follows:
    1. For the passed in model, call `to_rag_doll`
    2. Create all of the physics entities as appropriate, by calling `create_static_body`, `attach_collider`, `create_impulse_joint`, etc. These APIs already exist in physics world. As each rigid body is built, compute and store the transform from the bone’s joint origin to the collider center so `bone_frame_offsets` can later move poses back to joint space.
    3. These physics entities - along with the `RagDollInfo` that the model returns - will be stored in the `RagDoll` state.
- `remove_entity` ->  remove the rag doll entity completely from the physics
- `render` -> this will render all the ragdolls (producing sceneobjects and calling set_skinning_data). **This will be implemented in a later phase**

_Note:_ Integrating hitbox-derived collider shapes is deferred to a later phase; for Part 3 we'll use simple primitive bodies just to validate the hierarchy.

`Model`
- `Model` will add a new function `to_rag_doll`, that returns the `RagDollInfo`, porting over the bones and initial global transforms.  
- `Model` will add a new function `can_create_rag_doll` that only returns true for animated models.

For this phase, for `add_ragdoll`, we'll have a completely minimal implementation - we'll create _static_ (kinematic?) rigid bodies for all of the bones

To exercise this path in the `debug_ragdoll` scene we will:
1. Update `kill_spawned_entity` so that right after it queues the `ApplyDamage` effect it synchronously calls a new mission helper, `spawn_debug_ragdoll(entity_id)`.
2. `spawn_debug_ragdoll` will:
    - Look up the `Model` via `mission_core.id_to_model` and bail if `can_create_rag_doll` is false.
    - Pull the current `RuntimePropTransform` and `RuntimePropJointTransforms` for the dying entity so we have the final pose.
    - Call `model.to_rag_doll()` to get the `RagDollInfo`, then forward everything to `RagDollManager::add_ragdoll`.
3. `RagDollManager::add_ragdoll` will apply the 1-unit Y offset to the root transform before creating the static/kinematic bodies, store the source `EntityId` for cleanup, and keep the returned `RagDoll` in its map.

__Deliverable:__ When we run `debug_ragdoll` scene, once the entity is destroyed, the explicit call chain above spawns a static ragdoll (with joints) one unit above the corpse, proving the cloning flow works end-to-end.

__Status__: ✅ Implemented. `RagDollManager` now tracks per-entity ragdolls, `Model` exposes the skeleton data needed to seed them, and the `debug_ragdoll` scene calls `MissionCore::spawn_debug_ragdoll` after slaying the pipe hybrid, creating a chain of static joint bodies offset one unit above the corpse.

## Part 4: Connect model visualization

Goal: keep the ragdoll mesh in sync with the physics pose every frame and render it without relying on `AnimationPlayer`.

1. **Extend `RagDollManager::add_ragdoll`** so it clones the model’s scene objects up front (`model.build_scene_objects(identity_world)` or equivalent) and stores them in the `scene_objects` field. Each object should start with an identity world matrix so all motion comes from skinning data.
2. **Implement `RagDollManager::update(&mut self, physics_world: &PhysicsWorld)`** and call it immediately after `physics_world.step()` in `Mission::update` (before hitboxes/render collection):
    - For each ragdoll, pick a canonical root (pelvis) handle from `physics_entity_to_bone`. Call `physics_world.get_body_transform(handle)` to obtain an `Isometry`.
    - Convert the isometry to a `Matrix4`, multiply by the stored `bone_frame_offsets[root_joint]` (so the pelvis center lines up with the bone origin), and store it as the ragdoll’s root/global transform (applying any spawn offset captured during `add_ragdoll`).
    - For every bone, fetch its rigid body handle from `physics_entity_to_bone`. If present, call `get_body_transform`, convert to `Matrix4`, multiply by `bone_frame_offsets[joint_id]`, and write the result into `latest_global_transforms[joint_id]`. If a handle is missing (e.g., optional bones), fall back to the previous or initial transform so the array always stays populated.
    - Store the updated matrices so render code can consume them without touching Rapier again.
3. **Render hook**: when `Mission::gather_scene_objects` runs, iterate over all managed ragdolls and:
    - For each cached `scene_object`, set its world transform to identity (ragdoll meshes live in world space already) and call `set_skinning_data(&latest_global_transforms)`.
    - Submit the scene objects to the renderer or a dedicated debug pass. This bypasses `AnimationPlayer` entirely, proving the render pathway works with world-space matrices.
4. Add a debug overlay toggle (ex: `--debug-ragdoll-poses`) that renders both the rigid body centers and the corrected bone poses, drawing a short line segment that visualizes each `bone_frame_offset`. This confirms colliders that sit between joints still drive the right skinning transform.
5. Feed the same `latest_global_transforms` into `HitBoxManager` so post-mortem hit tests line up with the mesh. This can be a follow-up if needed.

__Deliverable__: spawning the debug ragdoll now shows the original creature mesh posed with the static physics rig, matching bone positions frame-to-frame.

__Status__: ✅ Implemented. `RagDollManager` clones the model scene objects, syncs their skinning data from physics each frame, and `MissionCore::render` always appends the ragdoll meshes so they’re visible alongside the static rigid-body hierarchy.

## Part 5: Full ragdoll implementation

Now that rendering is wired, convert the placeholder rig into a fully simulated ragdoll:

1. **Dynamic bodies**: switch the rigid bodies created in `RagDollManager::add_ragdoll` from `create_static_body` to `create_dynamic_body`, configuring mass, damping, and gravity scale per bone (lighter hands/feet, heavier torso). Keep `user_tag` set so debugging tools can identify them.
2. **Collider shapes**: derive capsules or oriented boxes from the existing hitbox data. Store per-bone offsets so the collider’s local origin matches the joint pivot used for skinning.
3. **Joint constraints**: for each parent/child bone pair, build `GenericJoint` or `SphericalJoint` descriptors with sensible angular limits (hinge knees, cone shoulders, twist limits for spine). Persist the resulting `ImpulseJointHandle`s so `remove_entity` can clean them up.
4. **Mission integration**: when a real creature dies (not just the debug scene), disable its original capsule/hitboxes, spawn a ragdoll entity via the manager, and route any lingering script references to the new entity if needed. Gate this behind a developer flag until tuning is complete.
5. **Update loop**: reuse the Part 4 `update` path so rendering and (optional) hitboxes follow the simulated pose every frame. Add stability helpers (e.g., wake/sleep control, optional pose blending) if the ragdoll jitters.
6. **Lifecycle**: ensure `Mission::remove_entity` tears down ragdolls by removing all stored rigid bodies, colliders, and joints. Consider adding a timeout/limit to despawn old corpses and avoid unbounded physics cost.

__Deliverable__: killing a creature causes its mesh to transition into a fully dynamic ragdoll that falls under gravity, collides with the level, and remains visually in sync without animator involvement.

__Status__: ✅ Implemented. `RagDollManager` now spawns dynamic Rapier bodies with spherical joints, syncs their poses every frame, and MissionCore renders them so the debug ragdoll collapses under physics instead of staying frozen.
