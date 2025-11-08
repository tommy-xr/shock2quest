use cgmath::{Deg, Matrix4, Quaternion, Rotation3, vec3};

use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script, ai_util,
    steering::{ChasePlayerSteeringStrategy, SteeringStrategy},
};

pub enum TurretState {
    Closed,
    Opening { progress: f32 },
    Closing { progress: f32 },
    Open,
}

impl TurretState {
    pub fn update(
        current_state: &TurretState,
        entity_id: EntityId,
        time: &Time,
        world: &World,
        physics: &PhysicsWorld,
    ) -> (TurretState, Effect) {
        let is_player_visible = ai_util::is_player_visible(entity_id, world, physics);

        match current_state {
            TurretState::Closed => {
                if is_player_visible {
                    (
                        TurretState::Opening { progress: 0.0 },
                        ai_util::play_positional_sound(
                            entity_id,
                            world,
                            None,
                            vec![("event", "activate")],
                        ),
                    )
                } else {
                    (TurretState::Closed, Effect::NoEffect)
                }
            }
            TurretState::Opening { progress } => {
                if *progress >= 1.0 {
                    (TurretState::Open, Effect::NoEffect)
                } else {
                    (
                        TurretState::Opening {
                            progress: progress + time.elapsed.as_secs_f32() / OPEN_TIME,
                        },
                        Effect::NoEffect,
                    )
                }
            }
            TurretState::Closing { progress } => {
                if *progress >= 1.0 {
                    (TurretState::Closed, Effect::NoEffect)
                } else {
                    (
                        TurretState::Closing {
                            progress: progress + time.elapsed.as_secs_f32() / OPEN_TIME,
                        },
                        Effect::NoEffect,
                    )
                }
            }
            TurretState::Open => {
                if !is_player_visible {
                    (
                        TurretState::Closing { progress: 0.0 },
                        ai_util::play_positional_sound(
                            entity_id,
                            world,
                            None,
                            vec![("event", "deactivate")],
                        ),
                    )
                } else {
                    (TurretState::Open, Effect::NoEffect)
                }
            }
        }
    }
}

const OPEN_TIME: f32 = 2.5;

pub struct TurretAI {
    // TODO: Implement logic (alert behavior, etc)
    next_fire: f32,
    initial_yaw: Deg<f32>, // keep track of the initial yaw, because the turret rotation is relative to it
    current_heading: Deg<f32>,
    steering: ChasePlayerSteeringStrategy,
    current_state: TurretState,
}

impl TurretAI {
    pub fn new() -> TurretAI {
        TurretAI {
            next_fire: 0.0,
            initial_yaw: Deg(0.0),
            steering: ChasePlayerSteeringStrategy,
            current_heading: Deg(0.0),
            current_state: TurretState::Closed,
        }
    }

    fn try_to_shoot(
        &mut self,
        time: &Time,
        world: &World,
        entity_id: EntityId,
        physics: &PhysicsWorld,
    ) -> Effect {
        let _quat = Quaternion::from_angle_x(Deg(time.total.as_secs_f32().sin() * 90.0));
        let fire_projectile = if self.next_fire < time.total.as_secs_f32() {
            self.next_fire = time.total.as_secs_f32() + 1.0;
            let rotation = Quaternion::from_angle_y(self.current_heading - self.initial_yaw);
            ai_util::fire_ranged_weapon(world, entity_id, rotation)
        } else {
            Effect::NoEffect
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

        Effect::combine(vec![fire_projectile, rotation_effect])
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
        let (new_state, state_eff) =
            TurretState::update(&self.current_state, entity_id, time, world, physics);
        self.current_state = new_state;

        let open_amount = match self.current_state {
            TurretState::Closed => 0.0,
            TurretState::Opening { progress } => progress,
            TurretState::Closing { progress } => 1.0 - progress,
            TurretState::Open => 1.0,
        };

        let cap_animation = Effect::SetJointTransform {
            entity_id,
            joint_id: 2,
            transform: Matrix4::from_translation(vec3(-0.75 * open_amount, 0.0, 0.0)),
        };

        let attack_eff = if matches!(self.current_state, TurretState::Open) {
            self.try_to_shoot(time, world, entity_id, physics)
        } else {
            Effect::NoEffect
        };

        Effect::combine(vec![cap_animation, state_eff, attack_eff])
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}
