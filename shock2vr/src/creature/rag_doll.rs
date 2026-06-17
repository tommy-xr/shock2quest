use std::collections::HashMap;

use cgmath::{Matrix4, Quaternion, Rotation, SquareMatrix, Vector3, vec3};
use collision::Aabb;
use dark::model::Model;
use engine::scene::SceneObject;
use rapier3d::{
    na::{Translation3, UnitQuaternion},
    prelude::{
        GenericJointBuilder, ImpulseJointHandle, Isometry, JointAxesMask, JointAxis,
        RigidBodyHandle, SharedShape,
    },
};
use shipyard::EntityId;

use crate::{
    physics::{CollisionGroup, PhysicsWorld, util::quat_to_nquat},
    util::{get_position_from_matrix, get_rotation_from_matrix, point3_to_vec3},
};

use super::JointLimit;

const DEFAULT_JOINT_RADIUS: f32 = 0.06;
/// Minimum collider half-extent, so degenerate joint AABBs (e.g. a joint with a
/// single skinned vertex) still get a valid, non-zero box.
const MIN_HALF_EXTENT: f32 = DEFAULT_JOINT_RADIUS;
/// Linear/angular damping on ragdoll limb bodies so they bleed off momentum and
/// settle instead of spinning or flailing forever (limbs out of world contact
/// have nothing else to slow them).
const LINEAR_DAMPING: f32 = 0.5;
const ANGULAR_DAMPING: f32 = 2.0;
/// Target mass for every ragdoll limb body. Densities are derived per-collider to
/// hit this, keeping connected-body mass ratios near 1:1 for solver stability.
const TARGET_BODY_MASS: f32 = 1.0;
/// Floor on collider volume when deriving density, to avoid div-by-zero / huge
/// density on degenerate shapes.
const MIN_VOLUME: f32 = 1e-4;
/// Uniform per-axis angular limit (radians, ~60°) for the limb ball joints,
/// measured from the bind/rest pose. Keeps the rig from folding/twisting through
/// itself. Per-bone limit profiles (hinge knees, cone shoulders) are a follow-up.
const JOINT_CONE_LIMIT: f32 = 1.05;

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
                }
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
        joint_limits: &HashMap<u32, JointLimit>,
        physics: &mut PhysicsWorld,
    ) -> bool {
        if !model.can_create_rag_doll() {
            return false;
        }

        let (bones, _) = match model.ragdoll_source() {
            Some(data) => data,
            None => return false,
        };

        // Per-joint bind-pose AABBs (model space), the same data that drives the
        // damage hitboxes. We size each limb's collider from its joint's box so
        // limbs have real volume/length instead of being point-like balls.
        let hit_boxes = model.get_hit_boxes();

        let offset_transform = Matrix4::from_translation(root_offset) * root_transform;
        let mut world_joint_transforms = [Matrix4::identity(); 40];
        for bone in &bones {
            let idx = bone.joint_id as usize;
            if idx < world_joint_transforms.len() {
                world_joint_transforms[idx] = offset_transform * joint_transforms[idx];
            }
        }

        self.remove_entity(entity_id, physics);

        let mut body_handles = Vec::new();
        let mut joint_handles = Vec::new();
        let mut joint_to_body = HashMap::new();
        let mut bone_offsets = HashMap::new();
        let mut joint_positions = vec![Vector3::new(0.0, 0.0, 0.0); world_joint_transforms.len()];

        for bone in &bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= world_joint_transforms.len() {
                continue;
            }

            let world_matrix = world_joint_transforms[joint_idx];
            let pos_vec = point3_to_vec3(get_position_from_matrix(&world_matrix));
            joint_positions[joint_idx] = pos_vec;
            let rotation = get_rotation_from_matrix(&world_matrix);
            let isometry = isometry_from_parts(pos_vec, rotation);

            let handle = physics.create_dynamic_body(isometry, Some(entity_id));
            physics.set_body_damping(handle, LINEAR_DAMPING, ANGULAR_DAMPING);

            // Size the collider from the joint's hitbox AABB when available: a
            // cuboid spanning the box, offset to the box center (in joint-local
            // space, matching how HitBoxManager places damage hitboxes). The body
            // origin stays at the joint, so skinning is unaffected. Fall back to a
            // small ball for joints with no hitbox (no skinned verts).
            match hit_boxes.get(&(bone.joint_id as u32)) {
                Some(bbox) => {
                    let dim = bbox.dim();
                    let center = bbox.center();
                    let half_extents = vec3(
                        (dim.x.abs() * 0.5).max(MIN_HALF_EXTENT),
                        (dim.y.abs() * 0.5).max(MIN_HALF_EXTENT),
                        (dim.z.abs() * 0.5).max(MIN_HALF_EXTENT),
                    );
                    // Density chosen so every limb has ~TARGET_BODY_MASS regardless
                    // of box size: an impulse-jointed chain is unstable when mass
                    // ratios between connected bodies are large (a tiny extremity
                    // box jointed to a big torso box spins up). Inertia still scales
                    // with the box's shape.
                    let volume = 8.0 * half_extents.x * half_extents.y * half_extents.z;
                    let density = TARGET_BODY_MASS / volume.max(MIN_VOLUME);
                    physics.attach_collider_with_offset(
                        handle,
                        SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z),
                        vec3(center.x, center.y, center.z),
                        density,
                        CollisionGroup::ragdoll(),
                    );
                }
                None => {
                    let r = DEFAULT_JOINT_RADIUS;
                    let volume = 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
                    let density = TARGET_BODY_MASS / volume.max(MIN_VOLUME);
                    physics.attach_collider(
                        handle,
                        SharedShape::ball(r),
                        density,
                        CollisionGroup::ragdoll(),
                    );
                }
            }

            joint_to_body.insert(bone.joint_id as u32, handle);
            bone_offsets.insert(bone.joint_id as u32, Matrix4::identity());
            body_handles.push(handle);
        }

        for bone in &bones {
            if let Some(parent_id) = bone.parent_id {
                let parent_handle = match joint_to_body.get(&(parent_id as u32)) {
                    Some(handle) => *handle,
                    None => continue,
                };
                let child_handle = match joint_to_body.get(&(bone.joint_id as u32)) {
                    Some(handle) => *handle,
                    None => continue,
                };

                let parent_idx = parent_id as usize;
                let child_idx = bone.joint_id as usize;
                if parent_idx >= joint_positions.len() || child_idx >= joint_positions.len() {
                    continue;
                }

                let parent_pos = joint_positions[parent_idx];
                let child_pos = joint_positions[child_idx];
                let parent_world = world_joint_transforms[parent_idx];
                let parent_rot = get_rotation_from_matrix(&parent_world);
                let child_world = world_joint_transforms[child_idx];
                let child_rot = get_rotation_from_matrix(&child_world);
                let child_to_parent = parent_pos - child_pos;
                let child_local_anchor = child_rot.conjugate().rotate_vector(child_to_parent);

                // Ball joint (translation locked) with angular limits so the body
                // can't fold/twist through itself.
                //
                // The limits are measured relative to the joints' local frames, so
                // they only behave if "zero angle" corresponds to the bind/rest
                // pose. We align both frames to the child's rest world orientation:
                //   frame1 (parent-local) rotation = R_parent^-1 * R_child
                //   frame2 (child-local)  rotation = identity
                // At spawn, R_parent * frame1 == R_child == R_child * frame2, so the
                // relative angle is exactly 0 and within limits - no energy is
                // injected (the bug we hit when limits used identity frames).
                let frame1_rot = quat_to_nquat(parent_rot.invert() * child_rot);
                let frame1 = Isometry::from_parts(Translation3::new(0.0, 0.0, 0.0), frame1_rot);
                let frame2 = Isometry::from_parts(
                    Translation3::new(
                        child_local_anchor.x,
                        child_local_anchor.y,
                        child_local_anchor.z,
                    ),
                    UnitQuaternion::identity(),
                );
                // Per-bone cone limit from the creature definition (e.g. tight for
                // head/neck/spine, wide for shoulders/hips), falling back to a
                // uniform default for joints/creatures without a profile.
                let cone = joint_limits
                    .get(&(bone.joint_id as u32))
                    .map(|limit| limit.cone)
                    .unwrap_or(JOINT_CONE_LIMIT);
                let joint = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                    .local_frame1(frame1)
                    .local_frame2(frame2)
                    .limits(JointAxis::AngX, [-cone, cone])
                    .limits(JointAxis::AngY, [-cone, cone])
                    .limits(JointAxis::AngZ, [-cone, cone])
                    .build();
                let handle = physics.create_impulse_joint(parent_handle, child_handle, joint);
                joint_handles.push(handle);
            }
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

    pub fn get_ragdoll_bodies(&self) -> Vec<RigidBodyHandle> {
        let mut all_bodies = Vec::new();
        for ragdoll in self.ragdolls.values() {
            all_bodies.extend(&ragdoll.physics_bodies);
        }
        all_bodies
    }

    pub fn get_first_ragdoll_body(&self) -> Option<RigidBodyHandle> {
        self.ragdolls
            .values()
            .next()
            .and_then(|ragdoll| ragdoll.physics_bodies.first())
            .copied()
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
