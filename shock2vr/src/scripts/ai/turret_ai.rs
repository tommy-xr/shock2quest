use cgmath::{vec3, vec4, Deg, Matrix4, Quaternion, Rotation3};
use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{ai_util, Effect, MessagePayload, Script};
pub struct TurretAI {
    // TODO: Implement logic (alert behavior, etc)
    next_fire: f32,
}

impl TurretAI {
    pub fn new() -> TurretAI {
        TurretAI { next_fire: 0.0 }
    }
}

impl Script for TurretAI {
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
        // let quat = Quaternion::from_angle_x(Deg(time.total.as_secs_f32().sin() * 90.0));
        let fire_projectile = if self.next_fire < time.total.as_secs_f32() {
            self.next_fire = time.total.as_secs_f32() + 1.0;
            ai_util::fire_ranged_weapon(world, entity_id)
        } else {
            Effect::NoEffect
        };

        let cap_animation = Effect::SetJointTransform {
            entity_id,
            joint_id: 2,
            transform: Matrix4::from_translation(vec3(-1.0, 0.0, 0.0)),
        };

        Effect::combine(vec![fire_projectile, cap_animation])
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
