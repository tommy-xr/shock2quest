
# Ragdoll Deaths Implementation Plan

## Goals

- Replace the current canned death animation with physics-driven ragdolls for humanoid creatures.
- Spawn a dedicated ragdoll entity on death that inherits the deceased model pose, then simulate it using Rapier rigid bodies and joints.
- Keep visual skinning and hit detection in sync with the simulated skeleton so the corpse interacts believably with the world.

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

## Part 2: Add debug visualization for skeletons
1. For desktop_runtime, add a `--debug-skeletons` command.
2. Add a function to ss2_skeleton.rs that is `debug_draw()`. This function should return a scene object for each bone, lines that connect bones to show the parent-child relationship
    a. To accomplish this, we'll iterate across the `bones` vec, which includes the relationship and the index (`joint_id`). From there, we'll place a sphere scene object at `global_transforms[bone.joint_id]`, and draw a line between `global_transforms[bone.joint_id]` and `global_transforms[bone.parent_id]]
3. Add a function to `model.rs` that is `draw_debug_skeleton()` - for an animated model, this 
4. When `--debug-skeletons` is active, draw all active skeletons in `mission_core` for each model

Deliverable: We can run a mission with `debug_skeletons` and see the skeleton and parent-child relationship visualized. This ensures we understand the world-space transforms and the parent-child relationship of the hierarchy.

## Part 3: Clone the skeleton

The goal for this deliverable is, when we run the `debug_ragdoll` scene, once the applydamage effect is done, we _clone_ the skeleton into a static ragdoll 1 unit above the current item. This will allow us to visualize that the layout is correct.

To do this, we'll create a new `ragdoll_manager` and `ragdoll` struct. We'll create this in shock2vr/src/creature/rag_doll.rs - the closet parallel will be the creature/hit_boxes.rs module.

The `RagDollManager` will be instantiated and owned by `mission_core`, and will be responsible for creating the ragdolls. It will store a `HashMap` of `<EntityId, RagDoll>`. 

`RagDoll`
- `bones: Vec<Bone>` - an array of bones, to understand the parent/child relationships
- `initial_global_transforms: Vec<Matrix4>` - the initial global transforms 

`RagDollManager`
- `new` -> create an empty instance
- `update` -> update the rag doll manager. For each managed ragdoll, we'll synchronize the _global_ (world) positions. **This will be implemented in a a later phase**
- `add_ragdoll` -> given an entity, model, and physics world, this will add a ragdoll. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints
- `remove_entity` ->  remove the rag doll entity completely from the physics
- `render` -> this will render all the ragdolls (producing sceneobjects and calling set_skinning_data). **This will be implemented in a later phase**

For this phase, for `add_ragdoll`, we'll have a completely minimal implementation - we'll create _static_ (kinematic?) rigid bodies for all of the bones

__Deliverable:__ When we run `debug_ragdoll` scene, once the entity is destroyed, we'll see a static ragdoll of the skeleton with appropriate joints, where it was standing.

## Part 4: Connect model visualization

TBD

## Part 5: Full ragdoll implementation

TBD

