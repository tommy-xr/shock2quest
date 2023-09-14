use std::cell::RefCell;

use cgmath::{vec3, vec4, Deg, Matrix4, Quaternion, Rotation3};
use dark::{
    motion::{MotionFlags, MotionQueryItem},
    SCALE_FACTOR,
};
use shipyard::{EntityId, World};

use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    time::Time,
};

use super::{
    ai_util::*,
    behavior::*,
    steering::{Steering, SteeringOutput},
    Effect, Message, MessagePayload, Script,
};
pub struct CameraAI {
    // last_hit_sensor: Option<EntityId>,
    // current_behavior: Box<RefCell<dyn Behavior>>,
    // current_heading: Deg<f32>,
    // is_dead: bool,
    // took_damage: bool,
    // animation_seq: u32,
}

impl CameraAI {
    pub fn new() -> CameraAI {
        CameraAI {
            // is_dead: false,
            // took_damage: false,
            // //current_behavior: Box::new(RefCell::new(MeleeAttackBehavior)),
            // current_behavior: Box::new(RefCell::new(ChaseBehavior::new())),
            // current_heading: Deg(0.0),
            // animation_seq: 0,
            // last_hit_sensor: None,
        }
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
        // Effect::SetJointTransform {
        //     entity_id,
        //     joint_id: 1,
        //     transform: quat.into(),
        // }
        Effect::SetJointTransform {
            entity_id,
            joint_id: 2,
            transform: Matrix4::from_translation(vec3(
                time.total.as_secs_f32().sin() - 1.0,
                0.0,
                0.0,
            )),
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
