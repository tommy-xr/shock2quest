use cgmath::{Deg, Matrix4, Quaternion, Rotation3, vec3, vec4};
use dark::properties::{AIAlertLevel, PropAIAlertCap, PropAIAwareDelay, PropPosition};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::DebugOptions, physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script, ai_util,
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
        let is_visible = ai_util::is_player_visible(entity_id, world, physics);

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

        // Existing turret state machine (unchanged behavior)
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

        // Debug visualization
        let debug_eff = draw_debug_turret_alertness(world, entity_id, &self.alertness, is_visible);

        Effect::combine(vec![
            alertness_effect,
            cap_animation,
            state_eff,
            attack_eff,
            debug_eff,
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

/// Draw debug visualization showing turret alertness state
fn draw_debug_turret_alertness(
    world: &World,
    entity_id: EntityId,
    alertness: &AlertnessState,
    is_visible: bool,
) -> Effect {
    // Check if debug_ai is enabled
    let debug_options = world.borrow::<UniqueView<DebugOptions>>().ok();
    if !debug_options.map(|d| d.debug_ai).unwrap_or(false) {
        return Effect::NoEffect;
    }

    let v_pos = world.borrow::<View<PropPosition>>().ok();
    if v_pos.is_none() {
        return Effect::NoEffect;
    }

    let v_pos = v_pos.unwrap();
    if let Ok(pose) = v_pos.get(entity_id) {
        let base_pos = pose.position;

        // Color based on alertness level
        let level_color = match alertness.current_level {
            AIAlertLevel::Lowest => vec4(0.0, 0.5, 0.0, 1.0), // Dark green
            AIAlertLevel::Low => vec4(0.5, 0.5, 0.0, 1.0),    // Yellow-green
            AIAlertLevel::Moderate => vec4(1.0, 0.5, 0.0, 1.0), // Orange
            AIAlertLevel::High => vec4(1.0, 0.0, 0.0, 1.0),   // Red
        };

        // Visibility indicator color
        let vis_color = if is_visible {
            vec4(0.0, 1.0, 0.0, 1.0) // Green = visible
        } else {
            vec4(0.5, 0.5, 0.5, 1.0) // Gray = not visible
        };

        // Draw alertness level indicator (vertical bar)
        let level_height = match alertness.current_level {
            AIAlertLevel::Lowest => 0.25,
            AIAlertLevel::Low => 0.5,
            AIAlertLevel::Moderate => 0.75,
            AIAlertLevel::High => 1.0,
        };

        let bar_base = base_pos + vec3(0.5, 0.0, 0.0);
        let bar_top = bar_base + vec3(0.0, level_height, 0.0);

        // Draw visibility indicator (horizontal line)
        let vis_base = base_pos + vec3(-0.5, 0.5, 0.0);
        let vis_end = vis_base + vec3(0.0, 0.0, 0.5);

        return Effect::DrawDebugLines {
            lines: vec![
                // Alertness level bar
                (
                    cgmath::point3(bar_base.x, bar_base.y, bar_base.z),
                    cgmath::point3(bar_top.x, bar_top.y, bar_top.z),
                    level_color,
                ),
                // Visibility indicator
                (
                    cgmath::point3(vis_base.x, vis_base.y, vis_base.z),
                    cgmath::point3(vis_end.x, vis_end.y, vis_end.z),
                    vis_color,
                ),
            ],
        };
    }

    Effect::NoEffect
}
