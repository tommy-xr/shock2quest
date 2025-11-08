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
const MIN_HITBOX_HALF_EXTENT: f32 = 0.01;

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
        physics: &mut PhysicsWorld,
    ) -> bool {
        if !model.can_create_rag_doll() {
            return false;
        }

        let (bones, _) = match model.ragdoll_source() {
            Some(data) => data,
            None => return false,
        };
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
        let mut body_world_transforms = HashMap::new();

        for bone in &bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= world_joint_transforms.len() {
                continue;
            }

            let world_joint = world_joint_transforms[joint_idx];
            let mut body_matrix = world_joint;
            let mut collider_shape = SharedShape::ball(DEFAULT_JOINT_RADIUS);
            let mut offset_matrix = Matrix4::identity();

            if let Some(bbox) = hit_boxes.get(&(bone.joint_id as u32)) {
                let center_translation = Matrix4::from_translation(bbox.center().to_vec());
                body_matrix = world_joint * center_translation;
                offset_matrix = body_matrix
                    .invert()
                    .map(|inv| inv * world_joint)
                    .unwrap_or_else(Matrix4::identity);
                let sizes = bbox.dim();
                let half_extents = vec3(
                    (sizes.x / 2.0).max(MIN_HITBOX_HALF_EXTENT),
                    (sizes.y / 2.0).max(MIN_HITBOX_HALF_EXTENT),
                    (sizes.z / 2.0).max(MIN_HITBOX_HALF_EXTENT),
                );
                collider_shape =
                    SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z);
            }

            let position = point3_to_vec3(get_position_from_matrix(&body_matrix));
            let rotation = get_rotation_from_matrix(&body_matrix);
            let isometry = isometry_from_parts(position, rotation);

            let handle = physics.create_dynamic_body(isometry, Some(entity_id));
            physics.attach_collider(handle, collider_shape, 1.0, CollisionGroup::selectable());

            joint_to_body.insert(bone.joint_id as u32, handle);
            bone_offsets.insert(bone.joint_id as u32, offset_matrix);
            body_world_transforms.insert(bone.joint_id as u32, body_matrix);
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
                if parent_idx >= world_joint_transforms.len() {
                    continue;
                }

                let parent_body_matrix = match body_world_transforms.get(&(parent_id as u32)) {
                    Some(matrix) => *matrix,
                    None => continue,
                };
                let child_body_matrix = match body_world_transforms.get(&(bone.joint_id as u32)) {
                    Some(matrix) => *matrix,
                    None => continue,
                };

                let parent_joint_world = world_joint_transforms[parent_idx];
                let pivot_world = point3_to_vec3(get_position_from_matrix(&parent_joint_world));
                let parent_body_pos = point3_to_vec3(get_position_from_matrix(&parent_body_matrix));
                let child_body_pos = point3_to_vec3(get_position_from_matrix(&child_body_matrix));
                let parent_rot = get_rotation_from_matrix(&parent_body_matrix);
                let child_rot = get_rotation_from_matrix(&child_body_matrix);

                let parent_anchor = parent_rot
                    .conjugate()
                    .rotate_vector(pivot_world - parent_body_pos);
                let child_anchor = child_rot
                    .conjugate()
                    .rotate_vector(pivot_world - child_body_pos);

                let joint = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                    .local_anchor1(Point3::new(
                        parent_anchor.x,
                        parent_anchor.y,
                        parent_anchor.z,
                    ))
                    .local_anchor2(Point3::new(child_anchor.x, child_anchor.y, child_anchor.z))
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
