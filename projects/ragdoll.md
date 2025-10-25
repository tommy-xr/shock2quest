# Ragdoll Deaths Implementation Plan

## Goals

- Replace the current canned death animation with physics-driven ragdolls for humanoid creatures.
- Spawn a dedicated ragdoll entity on death that inherits the deceased model pose, then simulate it using Rapier rigid bodies and joints.
- Keep visual skinning and hit detection in sync with the simulated skeleton so the corpse interacts believably with the world.

## Current Architecture Notes

- **Physics core** – `PhysicsWorld` (`shock2vr/src/physics/mod.rs:34` … `:620`) already owns the Rapier sets (`RigidBodySet`, `ColliderSet`, `ImpulseJointSet`, `MultibodyJointSet`). It exposes helpers such as `add_dynamic`/`add_kinematic` (`:382`, `:457`) that always tie one `EntityId` to one `RigidBodyHandle` through `entity_id_to_body`. There is no facility yet to:
  - Build networks of bodies for a single logical entity.
  - Author joints (e.g. spherical or generic constraints) between bodies.
  - Query rigid body transforms without an `EntityId`.
  - Update colliders that require non-uniform orientations/offsets.
- **Hitbox generation** – The `HitBoxManager` (`shock2vr/src/creature/hit_boxes.rs:28`) creates a hidden entity per joint. Each frame it multiplies the owner’s `RuntimePropTransform` and `RuntimePropJointTransforms` to place kinematic box colliders (`PhysicsWorld::add_kinematic`). These colliders follow the animated pose and are removed when the parent entity is destroyed (`:218`).
- **Death flow** – Animated monsters transition to `DeadBehavior` when `is_killed` returns true (`shock2vr/src/scripts/ai/animated_monster_ai.rs:333`). The behavior queues the “crumple” animation but never touches physics, so the corpse remains as a locked capsule.
- **Mission update loop** – `Mission::update` (`shock2vr/src/mission/mod.rs:376`) advances animation first, then refreshes hitboxes, then synchronizes physics transforms into `RuntimePropTransform` (`:480`), and finally dispatches script effects. Rendering collects models at the end (`:1407`), using the active `AnimationPlayer` to drive skinning.

### Observations

- We can reuse Rapier’s `ImpulseJointSet` for ragdoll constraints; we already carry it alongside the pipeline (`shock2vr/src/physics/mod.rs:602`).
- Hitboxes give us per-joint bounding boxes sized for damage; they are a natural starting point for ragdoll body shapes, though they currently live as separate ECS entities.
- Animation playback writes joint transforms into `RuntimePropJointTransforms` (`shock2vr/src/mission/mod.rs:522`). Once a creature dies we need to stop pushing animation data and instead fill that array from physics, otherwise the renderer and hitboxes will keep snapping back to animated poses.

## Proposed Ragdoll Flow

1. Detect death (script/AI) and gather the creature’s final world-space joint transforms plus the current `Model` skeleton.
2. Spawn a new “ragdoll corpse” entity that owns the visual model, a ragdoll runtime component, and (optionally) copied display props (`PropPosition`, `RuntimePropTransform`, etc.).
3. Build a ragdoll rig inside `PhysicsWorld`: create dynamic bodies for each joint we want to simulate, size colliders from hitbox AABBs, and connect them using joint definitions derived from the skeleton hierarchy.
4. Replace the original creature capsule with the ragdoll rig (remove or disable the old rigid body + scripts).
5. During each mission update, read the ragdoll physics pose, rebuild `RuntimePropJointTransforms` for the ragdoll entity, update its global transform, and optionally drive simplified hitboxes for lingering collision queries.

## Detailed Implementation Plan

### 1. Extend `PhysicsWorld` with ragdoll utilities

- Add low-level constructors to fabricate dynamic rigid bodies and colliders without requiring an `EntityId`. Possible APIs:
  - `create_dynamic_body(&mut self, isometry: Isometry<Real>, user_tag: Option<EntityId>) -> RigidBodyHandle`
  - `attach_collider(&mut self, handle: RigidBodyHandle, shape: ColliderBuilder, density: f32)`
- Provide helpers to author joints: expose a `create_impulse_joint(parent, child, joint_params)` wrapper that inserts into `impulse_joint_set` and stores the resulting `ImpulseJointHandle`.
- Add getters for raw body transforms (`RigidBodyHandle -> Isometry`) and per-collider AABBs so we can sample limb poses.
- Track ragdoll-owned handles for cleanup. A new map such as `ragdoll_handles: HashMap<EntityId, Vec<RigidBodyHandle>>` managed entirely within `PhysicsWorld` keeps removal localized.
- Ensure `remove(entity_id)` tears down any ragdoll handles tied to the ragdoll entity as well as its root body to avoid leaks.

### 2. Introduce ragdoll data structures in gameplay code

- Define a `RuntimePropRagdoll` component that records:
  - Parent creature entity (for bookkeeping).
  - `root_joint_id` and hierarchy map (joint -> parent joint).
  - `joint_handles: HashMap<JointId, RigidBodyHandle>`.
  - `joint_offsets: HashMap<JointId, Matrix4<f32>>` capturing the difference between the rigid body frame (hitbox center) and skeleton joint origin at spawn time.
  - `constraint_handles: Vec<ImpulseJointHandle>` for later teardown.
- Add an accompanying manager (similar to `HitBoxManager`) that lives in `Mission` and owns any ragdoll bookkeeping (e.g. `id_to_ragdoll: HashMap<EntityId, RuntimePropRagdoll>`).
- Extend serialization guards (`RuntimePropDoNotSerialize`) so ragdoll entities never enter save files.

### 3. Build the ragdoll rig when a creature dies

- Hook into the death effect path before `Mission::remove_entity` runs:
  - extend `Effect::SlayEntity` handling (`shock2vr/src/mission/mod.rs:1193`) to detect creatures and invoke `spawn_ragdoll_from_entity`.
  1. Capture final pose: read `RuntimePropTransform` and `RuntimePropJointTransforms` for the dying entity.
  2. Clone/derive the entity’s `Model` so the ragdoll can reuse it for rendering. Remove the animation player entry for the ragdoll to avoid future animation updates.
  3. Remove the creature’s existing physics body (`PhysicsWorld::remove`) to stop capsule interactions.
  4. Instantiate the ragdoll entity in Shipyard with components:
     - `PropPosition` (root pelvis world transform).
     - `RuntimePropTransform` (root matrix).
     - `RuntimePropJointTransforms` (initial pose).
     - `RuntimePropRagdoll` (described above).
     - `RuntimePropDoNotSerialize`.
  5. For each joint with a hitbox (or a curated subset: pelvis, spine, head, upper/lower arms, upper/lower legs):
     - Derive a collider size from the hitbox AABB (`dark::model::Model::get_hit_boxes`).
     - Compute the joint’s world transform (root × joint × hitbox-center).
     - Create a dynamic rigid body at that transform via the new `PhysicsWorld` APIs, store the handle, and register the ragdoll entity as `user_data` if we want debug picking.
  6. Iterate skeleton hierarchy (`dark/src/ss2_skeleton.rs`) to find parent-child joint pairs and author constraints:
     - Use `GenericJoint` or `SphericalJoint` with angular limits tuned per joint (e.g., hinge for knees, cone for shoulders). Start with generous limits and refine later.
     - Align joint frames so angular axes respect the bone orientation; we can compute local frames by inverse-multiplying parent and child bind poses.
  7. Optionally add lighter colliders (sensors) for interaction queries if we still need per-limb hit detection post-mortem.

### 4. Drive rendering and hit detection from physics

- Update loop:
  - Add a `Mission::update_ragdolls` step after `physics.update` but before `hit_boxes.update` so ragdoll data is fresh before other systems run.
  - For each ragdoll entry:
    1. Read the root rigid body transform. Write it to `PropPosition`/`RuntimePropTransform` for the ragdoll entity. If we stored an offset between pelvis body and visual origin, apply it here.
    2. For every joint: fetch the rigid body transform, apply the precomputed inverse of the hitbox center offset, and express it relative to the root to fill `RuntimePropJointTransforms`.
    3. (Optional) Recreate simple kinematic colliders for hit detection by reusing the existing `HitBoxManager`; supply it with the ragdoll’s joint transforms rather than live animation data.
  - Skip animation updates for ragdoll entities by removing them from `id_to_animation_player`.
- Rendering adjustments:
  - Extend the scene gathering step (`shock2vr/src/mission/mod.rs:1425`) so animated models without an `AnimationPlayer` but with `RuntimePropJointTransforms` still supply skinning data. We can add a helper on `Model` (`to_animated_scene_objects_with_skinning`) that accepts the joint array directly rather than requiring an `AnimationPlayer`.

### 5. Cleanup and lifecycle management

- Ensure `Mission::remove_entity` recognizes ragdoll corpses:
  - Remove the ragdoll entry from its manager.
  - Ask `PhysicsWorld` to free stored rigid bodies, colliders, and joints.
  - Clear any lingering hitboxes.
- Consider lifetime controls (despawn after a timeout, allow script removal, etc.) as follow-up work.
- Provide debug visualization toggles through `PhysicsWorld::debug_render` so we can inspect ragdoll bones and joints while tuning constraints.

## Risks & Open Questions

- **Joint tuning** – We need empirical limits per joint; start with permissive ranges and iterate. It may be worth adding developer config (`projects/ragdoll.md` follow-up) for quick tuning.
- **Collider alignment** – Hitbox bounding boxes are axis-aligned in bind pose. Limbs rotated far from their rest pose may need oriented capsules instead of boxes to avoid jitter.
- **Performance** – Full-body ragdolls add many dynamic bodies; we may want to cap the number of active corpses or simplify the rig for minor NPCs.
- **Save/Load** – Current plan marks ragdolls as non-serializable; if mid-mission saves are required, we will need a serialization story later.
- **Interaction with scripts** – Some Dark scripts may expect the original entity to persist post-death. Verify before we entirely replace it with a ragdoll entity.

## Next Steps Checklist

1. Prototype `PhysicsWorld` ragdoll helpers and a minimal 2-bone chain to validate joint API choices.
2. Implement `RuntimePropRagdoll` and spawn pipeline for one creature archetype.
3. Hook pose synchronization into the mission update loop and confirm rendering matches physics.
4. Iterate on constraint tuning, collider shapes, and cleanup before rolling out to all creatures.
