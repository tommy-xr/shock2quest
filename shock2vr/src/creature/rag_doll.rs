use std::collections::HashMap;

use cgmath::{EuclideanSpace, Matrix4, Quaternion, Rotation, SquareMatrix, Vector3, vec3};
use collision::Aabb;
use dark::model::Model;
use engine::scene::SceneObject;
use rapier3d::{
    na::{Point3, Translation3},
    prelude::{
        GenericJointBuilder, ImpulseJointHandle, Isometry, JointAxesMask, RigidBodyHandle,
        SharedShape,
    },
};
use shipyard::EntityId;

use crate::{
    physics::{CollisionGroup, PhysicsWorld, util::quat_to_nquat},
    util::{get_position_from_matrix, get_rotation_from_matrix, point3_to_vec3},
};

const DEFAULT_JOINT_RADIUS: f32 = 0.06;

pub struct RagDoll {
    physics_bodies: Vec<RigidBodyHandle>,
    joint_handles: Vec<ImpulseJointHandle>,
    joint_to_body: HashMap<u32, RigidBodyHandle>,
    bone_frame_offsets: HashMap<u32, Matrix4<f32>>,
    latest_global_transforms: [Matrix4<f32>; 40],
    scene_objects: Vec<SceneObject>,
}

impl RagDoll {
    fn new(
        joint_to_body: HashMap<u32, RigidBodyHandle>,
        physics_bodies: Vec<RigidBodyHandle>,
        joint_handles: Vec<ImpulseJointHandle>,
        initial_world: [Matrix4<f32>; 40],
        bone_frame_offsets: HashMap<u32, Matrix4<f32>>,
        scene_objects: Vec<SceneObject>,
    ) -> Self {
        Self {
            physics_bodies,
            joint_handles,
            joint_to_body,
            bone_frame_offsets,
            latest_global_transforms: initial_world,
            scene_objects,
        }
    }

    fn update(&mut self, physics: &PhysicsWorld) {
        for (joint_id, handle) in &self.joint_to_body {
            if let Some(isometry) = physics.get_body_transform(*handle) {
                let world = matrix_from_isometry(isometry);
                let idx = *joint_id as usize;
                if idx < self.latest_global_transforms.len() {
                    let offset = self
                        .bone_frame_offsets
                        .get(joint_id)
                        .copied()
                        .unwrap_or_else(Matrix4::identity);
                    self.latest_global_transforms[idx] = world * offset;

                    // Debug Joint 8 specifically
                    if *joint_id == 8 {
                        let pos = point3_to_vec3(get_position_from_matrix(&world));
                        println!("Joint 8 physics position: {:?} (handle: {:?})", pos, handle);
                    }
                }
            } else {
                println!("WARNING: Could not get transform for joint {} handle {:?}", joint_id, handle);
            }
        }
    }

    fn renderables(&self) -> Vec<SceneObject> {
        self.scene_objects
            .iter()
            .map(|obj| {
                let mut clone = obj.clone();
                clone.set_transform(Matrix4::identity());
                clone.set_skinning_data(self.latest_global_transforms);
                clone
            })
            .collect()
    }
}

pub struct RagDollManager {
    ragdolls: HashMap<EntityId, RagDoll>,
}

impl RagDollManager {
    pub fn new() -> Self {
        Self {
            ragdolls: HashMap::new(),
        }
    }

    pub fn add_ragdoll(
        &mut self,
        entity_id: EntityId,
        model: &Model,
        root_transform: Matrix4<f32>,
        joint_transforms: &[Matrix4<f32>; 40],
        root_offset: Vector3<f32>,
        physics: &mut PhysicsWorld,
    ) -> bool {
        if !model.can_create_rag_doll() {
            return false;
        }

        let (bones, _) = match model.ragdoll_source() {
            Some(data) => data,
            None => return false,
        };

        let offset_transform = Matrix4::from_translation(root_offset) * root_transform;
        let mut world_joint_transforms = [Matrix4::identity(); 40];
        for bone in &bones {
            let idx = bone.joint_id as usize;
            if idx < world_joint_transforms.len() {
                world_joint_transforms[idx] = offset_transform * joint_transforms[idx];
            }
        }

        self.remove_entity(entity_id, physics);

        // Get hitbox data from the model
        let hit_boxes = model.get_hit_boxes();

        let mut body_handles = Vec::new();
        let mut joint_handles = Vec::new();
        let mut joint_to_body = HashMap::new();
        let mut bone_offsets = HashMap::new();
        let mut joint_positions = vec![Vector3::new(0.0, 0.0, 0.0); world_joint_transforms.len()];
        let mut collider_positions = vec![Vector3::new(0.0, 0.0, 0.0); world_joint_transforms.len()];

        for bone in &bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= world_joint_transforms.len() {
                continue;
            }

            // Skip if we've already created a rigid body for this joint
            if joint_to_body.contains_key(&(bone.joint_id as u32)) {
                continue;
            }

            let world_matrix = world_joint_transforms[joint_idx];
            let joint_pos = point3_to_vec3(get_position_from_matrix(&world_matrix));
            joint_positions[joint_idx] = joint_pos;
            let rotation = get_rotation_from_matrix(&world_matrix);

            // Get hitbox for this joint
            let (collider_shape, collider_offset) = if let Some(hitbox) = hit_boxes.get(&(bone.joint_id as u32)) {
                let dimensions = hitbox.dim();
                let hitbox_center = hitbox.center().to_vec();

                if bone.joint_id == 8 {
                    println!("Joint 8 has hitbox: dimensions {:?}, center {:?}", dimensions, hitbox_center);
                }

                // Check for zero-dimension hitboxes and use default size
                let effective_dimensions = if dimensions.x <= 0.001 && dimensions.y <= 0.001 && dimensions.z <= 0.001 {
                    if bone.joint_id == 8 {
                        println!("Joint 8 hitbox has zero dimensions - using default size");
                    }
                    Vector3::new(0.3, 0.3, 0.3) // Default size for zero-dimension hitboxes
                } else {
                    dimensions
                };

                // Create a capsule or box collider from hitbox dimensions
                let collider = if effective_dimensions.x.max(effective_dimensions.z) > effective_dimensions.y * 0.8 {
                    // Use capsule for elongated hitboxes (limbs)
                    let radius = effective_dimensions.x.min(effective_dimensions.z) * 0.4;
                    let half_height = effective_dimensions.y * 0.4;
                    SharedShape::capsule_y(half_height, radius)
                } else {
                    // Use box for more cubic hitboxes (torso)
                    let half_extents = effective_dimensions * 0.4;
                    SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z)
                };

                // Calculate offset from joint to hitbox center
                let offset = Matrix4::from_translation(hitbox_center);
                (collider, offset)
            } else if bone.joint_id == 8 {
                // Always create a rigid body for the root bone (joint 8) even without hitbox
                println!("Joint 8 has no hitbox - creating default rigid body");
                let default_size = 0.15; // Reasonable default size for root
                (SharedShape::cuboid(default_size, default_size, default_size), Matrix4::identity())
            } else {
                // Skip other bones without hitboxes
                continue;
            };

            // Position the rigid body at the hitbox center, not the joint
            let collider_pos = joint_pos + point3_to_vec3(get_position_from_matrix(&collider_offset));
            collider_positions[joint_idx] = collider_pos;
            let isometry = isometry_from_parts(collider_pos, rotation);

            let handle = physics.create_dynamic_body(isometry, Some(entity_id));


            println!("Joint {} created as dynamic body with handle {:?}", bone.joint_id, handle);

            // Verify the body was actually created by checking if we can get its transform
            if let Some(transform) = physics.get_body_transform(handle) {
                println!("  ✓ Physics body verified for joint {}", bone.joint_id);
            } else {
                println!("  ✗ WARNING: Could not verify physics body for joint {}", bone.joint_id);
            }

            // Adjust mass based on bone type - heavier torso, lighter extremities
            let mass = if bone.joint_id == 0 {
                2.0 // Heavier root/torso
            } else if bone.joint_id < 5 {
                1.5 // Upper body bones
            } else {
                1.0 // Limbs and extremities
            };

            physics.attach_collider(
                handle,
                collider_shape,
                mass,
                CollisionGroup::selectable(),
            );

            println!("Created dynamic body for joint {} with mass {} at position {:?}", bone.joint_id, mass, collider_pos);

            joint_to_body.insert(bone.joint_id as u32, handle);
            // Store the inverse offset to convert from collider space back to joint space
            bone_offsets.insert(bone.joint_id as u32, collider_offset.invert().unwrap_or(Matrix4::identity()));
            body_handles.push(handle);
        }

        // Create a lookup table for efficient parent finding
        let mut bone_lookup = HashMap::new();
        for bone in &bones {
            bone_lookup.insert(bone.joint_id, bone);
        }

        // Track which joints already have constraints to prevent duplicates
        let mut joints_with_constraints = std::collections::HashSet::new();

        for bone in &bones {
            // Skip if this bone doesn't have a rigid body
            let child_handle = match joint_to_body.get(&(bone.joint_id as u32)) {
                Some(handle) => *handle,
                None => continue, // Child has no hitbox, skip this constraint
            };

            // Skip if we've already created a constraint for this joint
            if joints_with_constraints.contains(&bone.joint_id) {
                continue;
            }

            // Skip bones that have no parent (true root bones)
            if bone.parent_id.is_none() {
                continue;
            }

            if let Some(mut parent_id) = bone.parent_id {
                // Find the nearest ancestor that has a rigid body
                let mut parent_handle = None;
                let mut current_parent_id = parent_id;

                // Walk up the hierarchy to find an ancestor with a hitbox
                let mut search_depth = 0;
                for _ in 0..10 { // Limit search depth to prevent infinite loops
                    if let Some(handle) = joint_to_body.get(&(current_parent_id as u32)) {
                        parent_handle = Some(*handle);
                        parent_id = current_parent_id; // Update parent_id for position calculations
                        break;
                    }

                    // Move to the next parent up the hierarchy
                    if let Some(ancestor_bone) = bone_lookup.get(&current_parent_id) {
                        if let Some(grandparent_id) = ancestor_bone.parent_id {
                            current_parent_id = grandparent_id;
                            search_depth += 1;

                            // Limit how far we search to prevent long-distance constraints
                            if search_depth > 3 {
                                break; // Too far up the hierarchy
                            }
                        } else {
                            break; // Reached root without finding a parent with hitbox
                        }
                    } else {
                        break; // Invalid joint ID
                    }
                }

                let parent_handle = match parent_handle {
                    Some(handle) => handle,
                    None => continue, // No ancestor with hitbox found
                };

                // Mark this joint as having a constraint
                joints_with_constraints.insert(bone.joint_id);

                let parent_idx = parent_id as usize;
                let child_idx = bone.joint_id as usize;
                if parent_idx >= collider_positions.len() || child_idx >= collider_positions.len() {
                    continue;
                }

                let parent_collider_pos = collider_positions[parent_idx];
                let child_collider_pos = collider_positions[child_idx];
                let child_world = world_joint_transforms[child_idx];
                let child_rot = get_rotation_from_matrix(&child_world);
                let child_to_parent = parent_collider_pos - child_collider_pos;
                let child_local_anchor = child_rot.conjugate().rotate_vector(child_to_parent);

                // Debug output to track constraint creation
                println!("Creating constraint: child joint {} -> parent joint {} (search depth: {})", bone.joint_id, parent_id, search_depth);

                let joint = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                    .local_anchor1(Point3::origin())
                    .local_anchor2(Point3::new(
                        child_local_anchor.x,
                        child_local_anchor.y,
                        child_local_anchor.z,
                    ))
                    .build();
                let handle = physics.create_impulse_joint(parent_handle, child_handle, joint);
                joint_handles.push(handle);
            }
        }

        // Debug: Print all created rigid bodies
        println!("=== Created Rigid Bodies ===");
        for (joint_id, handle) in &joint_to_body {
            println!("Joint {} has rigid body handle {:?}", joint_id, handle);
        }
        println!("=== End Rigid Bodies ===");

        // Debug: Print complete bone hierarchy
        println!("=== Complete Bone Hierarchy ===");
        for bone in &bones {
            if let Some(parent_id) = bone.parent_id {
                println!("Bone {} -> parent {}", bone.joint_id, parent_id);
            } else {
                println!("Bone {} -> ROOT (no parent)", bone.joint_id);
            }
        }
        println!("=== End Bone Hierarchy ===");

        // Debug: Check which joints have no parent constraints
        println!("=== Joints without parent constraints (potential roots) ===");
        for (joint_id, _handle) in &joint_to_body {
            let mut has_parent_constraint = false;
            for bone in &bones {
                if bone.joint_id as u32 == *joint_id {
                    if bone.parent_id.is_some() && joints_with_constraints.contains(&(bone.joint_id as u32)) {
                        has_parent_constraint = true;
                        break;
                    }
                }
            }
            if !has_parent_constraint {
                println!("Joint {} has no parent constraint - acting as root!", joint_id);
                // Show why this joint has no parent constraint
                for bone in &bones {
                    if bone.joint_id as u32 == *joint_id {
                        if let Some(parent_id) = bone.parent_id {
                            if !joint_to_body.contains_key(&(parent_id as u32)) {
                                println!("  -> Parent {} has no rigid body (no hitbox)", parent_id);
                            }
                        } else {
                            println!("  -> This is the true skeleton root");
                        }
                        break;
                    }
                }
            }
        }
        println!("=== End Root Joints ===");

        // Apply impulse to the root joint (Joint 8) to initiate physics simulation
        if let Some(root_handle) = joint_to_body.get(&8) {
            physics.apply_impulse(*root_handle, vec3(0.0, -2.0, 0.0));
            println!("Applied downward impulse to root joint 8");
        }

        let ragdoll = RagDoll::new(
            joint_to_body,
            body_handles,
            joint_handles,
            world_joint_transforms,
            bone_offsets,
            model.clone_scene_objects(),
        );
        self.ragdolls.insert(entity_id, ragdoll);
        true
    }

    pub fn update(&mut self, physics: &PhysicsWorld) {
        for ragdoll in self.ragdolls.values_mut() {
            ragdoll.update(physics);
        }
    }

    pub fn render_scene_objects(&self) -> Vec<SceneObject> {
        let mut scene = Vec::new();
        for ragdoll in self.ragdolls.values() {
            scene.extend(ragdoll.renderables());
        }
        scene
    }

    pub fn remove_entity(&mut self, entity_id: EntityId, physics: &mut PhysicsWorld) {
        if let Some(ragdoll) = self.ragdolls.remove(&entity_id) {
            for joint in ragdoll.joint_handles {
                physics.remove_impulse_joint(joint);
            }
            for body in ragdoll.physics_bodies {
                physics.remove_rigid_body_handle(body);
            }
        }
    }

    pub fn has_ragdoll(&self, entity_id: EntityId) -> bool {
        self.ragdolls.contains_key(&entity_id)
    }
}

fn isometry_from_parts(position: Vector3<f32>, rotation: Quaternion<f32>) -> Isometry<f32> {
    Isometry::from_parts(
        Translation3::new(position.x, position.y, position.z),
        quat_to_nquat(rotation),
    )
}

fn matrix_from_isometry(iso: Isometry<f32>) -> Matrix4<f32> {
    let translation = Matrix4::from_translation(vec3(
        iso.translation.x,
        iso.translation.y,
        iso.translation.z,
    ));
    let rotation = Matrix4::from(Quaternion::new(
        iso.rotation.w,
        iso.rotation.i,
        iso.rotation.j,
        iso.rotation.k,
    ));
    translation * rotation
}
