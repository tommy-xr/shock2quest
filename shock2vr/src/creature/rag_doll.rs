use std::collections::HashMap;

use cgmath::{vec3, Matrix4, Quaternion, SquareMatrix, Vector3};
use dark::model::Model;
use engine::scene::SceneObject;
use rapier3d::{
    na::Translation3,
    prelude::{
        GenericJointBuilder, ImpulseJointHandle, Isometry, JointAxesMask, RigidBodyHandle,
        SharedShape,
    },
};
use shipyard::EntityId;

use crate::{
    physics::{util::quat_to_nquat, CollisionGroup, PhysicsWorld},
    util::{get_position_from_matrix, get_rotation_from_matrix, point3_to_vec3},
};

const DEFAULT_JOINT_RADIUS: f32 = 0.06;

pub struct RagDoll {
    physics_bodies: Vec<RigidBodyHandle>,
    joint_handles: Vec<ImpulseJointHandle>,
    joint_to_body: HashMap<u32, RigidBodyHandle>,
    latest_global_transforms: [Matrix4<f32>; 40],
    scene_objects: Vec<SceneObject>,
}

impl RagDoll {
    fn new(
        joint_to_body: HashMap<u32, RigidBodyHandle>,
        physics_bodies: Vec<RigidBodyHandle>,
        joint_handles: Vec<ImpulseJointHandle>,
        initial_world: [Matrix4<f32>; 40],
        scene_objects: Vec<SceneObject>,
    ) -> Self {
        Self {
            physics_bodies,
            joint_handles,
            joint_to_body,
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
                    self.latest_global_transforms[idx] = world;
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

        for bone in &bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= world_joint_transforms.len() {
                continue;
            }

            let world_matrix = world_joint_transforms[joint_idx];
            let position = point3_to_vec3(get_position_from_matrix(&world_matrix));
            let rotation = get_rotation_from_matrix(&world_matrix);
            let isometry = isometry_from_parts(position, rotation);

            let handle = physics.create_static_body(isometry, Some(entity_id));
            physics.attach_collider(
                handle,
                SharedShape::ball(DEFAULT_JOINT_RADIUS),
                1.0,
                CollisionGroup::selectable(),
            );

            joint_to_body.insert(bone.joint_id as u32, handle);
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

                let joint = GenericJointBuilder::new(JointAxesMask::LOCKED_FIXED_AXES).build();
                let handle = physics.create_impulse_joint(parent_handle, child_handle, joint);
                joint_handles.push(handle);
            }
        }

        let ragdoll = RagDoll::new(
            joint_to_body,
            body_handles,
            joint_handles,
            world_joint_transforms,
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
