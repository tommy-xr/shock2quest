
# Ragdoll Deaths Implementation Plan

## Goals

- Replace the current canned death animation with physics-driven ragdolls for humanoid creatures.
- Spawn a dedicated ragdoll entity on death that inherits the deceased model pose, then simulate it using Rapier rigid bodies and joints.
- Keep visual skinning and hit detection in sync with the simulated skeleton so the corpse interacts believably with the world.

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
