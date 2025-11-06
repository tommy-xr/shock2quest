
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

`RagDollInfo`
- `bones: Vec<Bone>` - an array of bones, to understand the parent/child relationships
- `initial_global_transforms: Vec<Matrix4>` - the initial global transforms 

`RagDoll` - state kept by rag doll manager
- `bones: Vec<Bone>` - an array of bones, to understand the parent/child relationships
- `initial_global_transforms: Vec<Matrix4>` - the initial global transforms 
- `physics_entities` - the list of physics entities that were created as part of the ragdoll. These may be created via `create_dynamic_body`, `attach_collider` and `create_impulse_joint`
- `physics_entity_to_bone: HashMap<JointId, PhysicsEntity>` - a dictionary that tracks the phsyics entity that should correspond to the transform. 
- `latest_global_transforms: Vec<Matrix4>` - the latest global transforms, which are synced from the physics entities. Initially, this will just be taken from initial_global_transforms.

`RagDollManager`
- `new` -> create an empty instance
- `update` -> update the rag doll manager. For each managed ragdoll, we'll synchronize the _global_ (world) positions. **This will be implemented in a a later phase**
- `add_ragdoll` -> given an entity, model, and physics world, this will add a ragdoll. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints. We'll have to create the appropriate physics entities given the skeleton (and hitboxes, potentially?), with proper constraints. The flow will be as follows:
    1. For the passed in model, call `to_rag_doll`
    2. Create all of the physics entities as appropriate, by calling `create_static_body`, `attach_collider`, `create_impulse_joint`, etc.
    3. These physics entities - along with the `RagDollInfo` that the model returns - will be stored in the `RagDoll` state.
- `remove_entity` ->  remove the rag doll entity completely from the physics
- `render` -> this will render all the ragdolls (producing sceneobjects and calling set_skinning_data). **This will be implemented in a later phase**

`Model`
- `Model` will add a new function `to_rag_doll`, that returns the `RagDollInfo`, porting over the bones and initial global transforms.  
- `Model` will add a new function `can_create_rag_doll` that only returns true for animated models.

For this phase, for `add_ragdoll`, we'll have a completely minimal implementation - we'll create _static_ (kinematic?) rigid bodies for all of the bones

__Deliverable:__ When we run `debug_ragdoll` scene, once the entity is destroyed, we'll see a static ragdoll of the skeleton with appropriate joints, where it was standing.

## Part 4: Connect model visualization

TBD, but the goal of this implementation is to verify we can properly connect the world-space physics bodies with rendering. In order to avoid the issues we ran into previously, we'll create the scene objects directly and call set_skinning_data with the _global_ transforms (and use an identity matrix for the world transform). This should avoid all the awkard coordinate transforms - we're relying on the fact that, for ss2 models, there is no bind pose, all of the parts are at the origin.

On each `update` for `RagDollManager`, we'll synchronize the transforms from the physics objects to `latest_global_transforms` for every RagDoll. This willr equire, for each bone, reading back the global transform of the physics entity in `physics_entity_to_bone`t with `get_position` and `get_rotation`, create a transform matrix

In order to accomplish this, we'll need to add a `model: Vec<SceneObject>` to `RagDoll`. Then, when we render, we'll iterate through each scene object, and call `set_skinning_data` with the `latest_global_transforms`.

## Part 5: Full ragdoll implementation

TBD, but convert the ragdoll entities from kinematic to real physics bodies, so we can finally see the ragdoll in all its glory.

