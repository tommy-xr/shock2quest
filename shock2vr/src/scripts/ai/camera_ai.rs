use cgmath::{vec3, vec4, Deg, Matrix4, Quaternion, Rotation3};
use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script};
pub struct CameraAI {
    // TODO: Implement logic (alert behavior, etc)
}

impl CameraAI {
    pub fn new() -> CameraAI {
        CameraAI {}
    }
}

impl Script for CameraAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        Effect::NoEffect
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let quat = Quaternion::from_angle_x(Deg(time.total.as_secs_f32().sin() * 90.0));
        Effect::SetJointTransform {
            entity_id,
            joint_id: 1,
            transform: quat.into(),
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}
