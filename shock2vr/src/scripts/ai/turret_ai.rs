use cgmath::{vec3, vec4, Deg, Matrix4, Quaternion, Rotation3};
use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    ai_util,
    steering::{ChasePlayerSteeringStrategy, SteeringStrategy},
    Effect, MessagePayload, Script,
};
pub struct TurretAI {
    // TODO: Implement logic (alert behavior, etc)
    next_fire: f32,
    initial_yaw: Deg<f32>, // keep track of the initial yaw, because the turret rotation is relative to it
    current_heading: Deg<f32>,
    steering: ChasePlayerSteeringStrategy,
}

impl TurretAI {
    pub fn new() -> TurretAI {
        TurretAI {
            next_fire: 0.0,
            initial_yaw: Deg(0.0),
            steering: ChasePlayerSteeringStrategy,
            current_heading: Deg(0.0),
        }
    }
}

impl Script for TurretAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.initial_yaw = ai_util::current_yaw(entity_id, world);
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
            let rotation = Quaternion::from_angle_y(self.current_heading - self.initial_yaw);
            ai_util::fire_ranged_weapon(world, entity_id, rotation)
        } else {
            Effect::NoEffect
        };

        let cap_animation = Effect::SetJointTransform {
            entity_id,
            joint_id: 2,
            transform: Matrix4::from_translation(vec3(-0.75, 0.0, 0.0)),
        };

        let maybe_desired_yaw =
            self.steering
                .steer(self.current_heading, world, physics, entity_id, time);

        let rotation_effect = {
            if let Some((steering_output, _effect)) = maybe_desired_yaw {
                self.current_heading = steering_output.desired_heading;
                let rotate = Quaternion::from_angle_x(Deg(self.initial_yaw.0
                    - self.current_heading.0
                    - 90.0));
                let rotate_animation = Effect::SetJointTransform {
                    entity_id,
                    joint_id: 1,
                    transform: rotate.into(),
                };

                Effect::combine(vec![rotate_animation, _effect])
            } else {
                Effect::NoEffect
            }
        };

        Effect::combine(vec![fire_projectile, cap_animation, rotation_effect])
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
