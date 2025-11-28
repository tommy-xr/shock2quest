use cgmath::{Deg, Matrix4, Quaternion, Rotation3, vec3};
use dark::properties::{AIAlertLevel, PropAIAlertCap, PropAIAwareDelay};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script,
    ai_debug_util::{self, AlertnessDebugConfig, FovDebugConfig},
    ai_util,
    alertness::{self, AlertnessState, AlertnessTimings},
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
        is_player_visible: bool,
    ) -> (TurretState, Effect) {
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

// Default timing constants for turrets (in seconds)
const DEFAULT_ESCALATE_SECONDS: f32 = 2.0;
const DEFAULT_DECAY_SECONDS: f32 = 4.0;

/// Configuration for turret alertness behavior
#[derive(Clone)]
struct TurretConfig {
    alert_cap: PropAIAlertCap,
    timings: AlertnessTimings,
}

pub struct TurretAI {
    next_fire: f32,
    initial_yaw: Deg<f32>,
    current_heading: Deg<f32>,
    steering: ChasePlayerSteeringStrategy,
    current_state: TurretState,
    /// Alertness state tracking
    alertness: AlertnessState,
    /// Alertness configuration (loaded from entity properties)
    config: Option<TurretConfig>,
}

impl TurretAI {
    pub fn new() -> TurretAI {
        TurretAI {
            next_fire: 0.0,
            initial_yaw: Deg(0.0),
            steering: ChasePlayerSteeringStrategy,
            current_heading: Deg(0.0),
            current_state: TurretState::Closed,
            alertness: AlertnessState::default(),
            config: None,
        }
    }

    fn build_config(world: &World, entity_id: EntityId) -> Option<TurretConfig> {
        let (v_alert_cap, v_aware_delay): (View<PropAIAlertCap>, View<PropAIAwareDelay>) =
            world.borrow().ok()?;

        let alert_cap = v_alert_cap
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAIAlertCap {
                max_level: AIAlertLevel::High,
                min_level: AIAlertLevel::Lowest,
                min_relax: AIAlertLevel::Low,
            });

        // Build default aware delay for turrets
        let default_aware_delay = PropAIAwareDelay {
            to_two: (DEFAULT_ESCALATE_SECONDS * 1000.0) as u32,
            to_three: (DEFAULT_ESCALATE_SECONDS * 1000.0) as u32,
            two_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
            three_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
            ignore_range: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
        };

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(default_aware_delay);

        let timings = AlertnessTimings::from_aware_delay(&aware_delay);

        Some(TurretConfig { alert_cap, timings })
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

        // Load alertness configuration
        self.config = Self::build_config(world, entity_id);

        // Initialize alertness state
        if let Some(config) = &self.config {
            let initial_level = alertness::clamp_level(AIAlertLevel::Lowest, &config.alert_cap);
            self.alertness = AlertnessState::new(initial_level);

            // Sync initial alertness to ECS
            return alertness::sync_alertness_effect(entity_id, &self.alertness);
        }

        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let delta = time.elapsed.as_secs_f32();

        // Turret FOV is 30 degrees half-angle (matches FovDebugConfig::turret())
        // Turret uses joint transforms for rotation, negate heading to match visual direction
        const TURRET_FOV_HALF_ANGLE: f32 = 30.0;
        let is_visible = ai_util::is_player_visible_in_fov(
            entity_id,
            world,
            physics,
            -self.current_heading,
            TURRET_FOV_HALF_ANGLE,
        );

        // Update alertness state
        let alertness_effect = if let Some(config) = &self.config {
            if let Some((_old_level, _new_level)) = alertness::process_alertness_update(
                &mut self.alertness,
                is_visible,
                delta,
                &config.timings,
                &config.alert_cap,
            ) {
                // Level changed - sync to ECS
                alertness::sync_alertness_effect(entity_id, &self.alertness)
            } else {
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        };

        // Existing turret state machine (now uses FOV-aware visibility)
        let (new_state, state_eff) =
            TurretState::update(&self.current_state, entity_id, time, world, is_visible);
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

        // Debug visualization - alertness bar
        let alertness_debug_eff = ai_debug_util::draw_debug_alertness(
            world,
            entity_id,
            &self.alertness,
            is_visible,
            &AlertnessDebugConfig::turret(),
        );

        // Debug visualization - FOV cone
        // Negate heading to match visibility check
        let fov_debug_eff = ai_debug_util::draw_debug_fov(
            world,
            entity_id,
            -self.current_heading,
            is_visible,
            &FovDebugConfig::turret(),
        );

        Effect::combine(vec![
            alertness_effect,
            cap_animation,
            state_eff,
            attack_eff,
            alertness_debug_eff,
            fov_debug_eff,
        ])
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
