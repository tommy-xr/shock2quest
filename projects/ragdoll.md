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


Unfortunately, we weren't able to implement this in our initial iteration.

## Claude's feedback: 


⏺ You're absolutely right - this ragdoll implementation has proven much more complex than anticipated. Let me analyze the key challenges
  and suggest architectural improvements.

  Biggest Challenges & Points of Confusion

  1. Coordinate System Complexity

  - Multiple transform spaces: World space, entity space, joint space, physics body space
  - Matrix multiplication order: Getting entity_relative_transform * rest.inverse_bind right
  - Joint offset calculations: Understanding what joint_offsets actually represents vs what constraints need

  2. Dark Engine Legacy Data Structures

  - Joint hierarchy confusion: CreatureDefinition remap vs actual skeleton hierarchy
  - Hitbox positioning: Unclear relationship between hitboxes and joint positions
  - Animation system integration: Complex interaction between AnimationPlayer and physics

  3. Physics Engine Integration

  - Rapier constraint anchors: Local vs world space anchor calculations
  - Body positioning: Physics bodies at hitbox centers vs joint positions
  - Constraint types: Spherical joints may not be the right choice

  4. Debugging Difficulty

  - Visual feedback gaps: Hard to see what's actually happening with constraints
  - Transform debugging: Matrix calculations are opaque
  - Physics visualization: No way to see constraint anchor points

  Architectural Improvements

  Option 1: Simplified Physics-Only Ragdolls

  Instead of trying to sync visual models, use physics bodies as the visual representation:

  // Replace visual model with debug physics body rendering
  pub struct SimpleRagdoll {
      pub physics_bodies: Vec<RigidBodyHandle>,
      pub constraints: Vec<ImpulseJointHandle>,
      // No visual model sync - just render the physics bodies directly
  }

  Benefits:
  - Eliminates transform synchronization complexity
  - Debugging is much easier (see exactly what physics sees)
  - Fewer coordinate systems to manage

  Option 2: Constraint-Free Ragdolls

  Use independent physics bodies without joint constraints:

  pub struct LooseRagdoll {
      pub body_parts: HashMap<u32, RigidBodyHandle>,
      // No constraints - just independent falling pieces
  }

  Benefits:
  - Much simpler physics setup
  - Still provides satisfying death effects
  - Eliminates constraint anchor calculations

  Option 3: Pre-Baked Ragdoll Data

  Create a tool to pre-calculate and validate ragdoll setups:

  // CLI tool: cargo run -p ragdoll_baker -- pipe_hybrid.bin
  pub struct BakedRagdollData {
      pub validated_anchors: Vec<(Point3<f32>, Point3<f32>)>,
      pub tested_constraints: Vec<ConstraintParams>,
      pub visual_sync_matrices: HashMap<u32, Matrix4<f32>>,
  }

  Benefits:
  - Move complexity to build-time tool
  - Runtime becomes simple data loading
  - Easier debugging and iteration

  Option 4: Physics Visualization Tools

  Add debug rendering to understand what's happening:

  pub struct RagdollDebugRenderer {
      pub show_constraint_anchors: bool,
      pub show_joint_hierarchies: bool,
      pub show_transform_spaces: bool,
  }

  Benefits:
  - Makes debugging much easier
  - Could reveal the actual issues
  - Useful for future physics work

  Recommended Next Steps

  Given the time investment vs complexity, I'd suggest:

  1. Immediate: Implement Option 1 (Physics-Only) for quick wins
  2. Medium-term: Add Option 4 (Debug Visualization) for future physics work
  3. Long-term: Consider Option 3 (Pre-Baked Data) for production quality

  The core issue seems to be that ragdoll physics requires deep understanding of the Dark Engine's animation system, which has many
  undocumented assumptions about coordinate spaces and transform hierarchies.

  Would you like me to implement the simplified physics-only approach, or would you prefer to explore the debug visualization tools first
   to better understand what's going wrong?

## Codex's feedback

• - I’ve exhausted the current iteration: even after driving the skinning overrides (shock2vr/src/mission/mission_core.rs:2512) and
    upgrading constraint anchors (shock2vr/src/mission/mission_core.rs:2231), the mesh still drifts into the bind pose and the link lengths
    collapse. The blockers remain the same—so I stopped short of more speculative surgery.
  - I’m convinced we need a cleaner architecture rather than more patches. Two concrete ideas:
      - Build a dedicated “ragdoll rig” source of truth alongside the loader: for each skeleton, precompute the bone hierarchy, rest-
        space offsets, collider geometry, and mass distribution once (in dark). That single structure could hand the game both the render
        skinning data and the physics setup, eliminating the ad‑hoc conversions we’re doing now.
      - Split animated/render rigs from physics rigs. Right now we push Rapier poses back through AnimationPlayer while update_animations
        overwrites those transforms every frame. A specific ragdoll system that bypasses AnimationPlayer (or tells update_animations to
        skip ragdolls) would stop the tug‑of‑war and keep the data flow clear.
  - The hardest parts here came from ambiguous coordinate spaces: hitbox IDs don’t line up with skeleton joints, joint_offsets store global
    matrices when the joints need local anchors, and the render pipeline expects global * inverse_bind. Those mismatches made every fix
    feel like guesswork. On top of that, several systems mutate the same components (RuntimePropJointTransforms) each frame, so it wasn’t
    obvious who “owns” the data.

  I’ll pause here so we can decide on the larger refactor path rather than continue iterating inside the current constraints.
