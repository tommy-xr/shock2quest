use cgmath::{point3, vec3, Deg, Quaternion, Rad, Rotation, Rotation3};
use dark::properties::{
    AIAlertLevel, PropAIAlertCap, PropAIAlertness, PropAIAwareDelay, PropAICamera, PropAIDevice,
    PropModelName, PropPosition,
};
use num_traits::{FromPrimitive, ToPrimitive};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{ai::ai_util, AIPropertyUpdate, Effect},
    time::Time,
};

use super::{MessagePayload, Script};

#[derive(Clone)]
struct CameraConfig {
    device: PropAIDevice,
    camera: PropAICamera,
    alert_cap: PropAIAlertCap,
    timings: CameraTimings,
    models: CameraModels,
    facing_epsilon: Deg<f32>,
}

#[derive(Clone)]
struct CameraTimings {
    to_two: f32,
    to_three: f32,
    two_reuse: f32,
    three_reuse: f32,
    ignore_range: f32,
}

#[derive(Clone)]
struct CameraModels {
    green: String,
    yellow: String,
    red: String,
}

struct CameraState {
    current_level: AIAlertLevel,
    peak_level: AIAlertLevel,
    visible_time: f32,
    hidden_time: f32,
    current_model: Option<String>,
    initial_yaw: Deg<f32>,
    current_offset: Deg<f32>,
    scan_min_offset: Deg<f32>,
    scan_max_offset: Deg<f32>,
    scan_direction: f32,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            current_level: AIAlertLevel::Lowest,
            peak_level: AIAlertLevel::Lowest,
            visible_time: 0.0,
            hidden_time: 0.0,
            current_model: None,
            initial_yaw: Deg(0.0),
            current_offset: Deg(0.0),
            scan_min_offset: Deg(-45.0),
            scan_max_offset: Deg(45.0),
            scan_direction: 1.0,
        }
    }
}

pub struct CameraAI {
    config: Option<CameraConfig>,
    state: CameraState,
}

impl CameraAI {
    pub fn new() -> CameraAI {
        CameraAI {
            config: None,
            state: CameraState::default(),
        }
    }

    fn combine_effects(effects: Vec<Effect>) -> Effect {
        match effects.len() {
            0 => Effect::NoEffect,
            1 => effects.into_iter().next().unwrap(),
            _ => Effect::Multiple(effects),
        }
    }

    fn build_config(world: &World, entity_id: EntityId) -> Option<(CameraConfig, CameraState)> {
        let initial_yaw = ai_util::current_yaw(entity_id, world);
        let (v_device, v_camera, v_alert_cap, v_alertness, v_aware_delay, v_model_name): (
            View<PropAIDevice>,
            View<PropAICamera>,
            View<PropAIAlertCap>,
            View<PropAIAlertness>,
            View<PropAIAwareDelay>,
            View<PropModelName>,
        ) = world
            .borrow::<(
                View<PropAIDevice>,
                View<PropAICamera>,
                View<PropAIAlertCap>,
                View<PropAIAlertness>,
                View<PropAIAwareDelay>,
                View<PropModelName>,
            )>()
            .ok()?;

        let device = v_device.get(entity_id).ok().cloned().or_else(|| {
            Some(PropAIDevice {
                joint_activate: 0,
                inactive_pos: 0.0,
                active_pos: 2.0,
                activate_speed: 0.1,
                joint_rotate: 1,
                facing_epsilon: 0.1,
                activate_rotate: false,
            })
        })?;

        let camera = v_camera
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAICamera {
                scan_angle_1: -180.0,
                scan_angle_2: 180.0,
                scan_speed: 0.05,
            });

        let alert_cap = v_alert_cap
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAIAlertCap {
                max_level: AIAlertLevel::High,
                min_level: AIAlertLevel::Lowest,
                min_relax: AIAlertLevel::Low,
            });

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAIAwareDelay {
                to_two: 750,
                to_three: 500,
                two_reuse: 12_000,
                three_reuse: 22_000,
                ignore_range: 9,
            });

        let base_model = v_model_name
            .get(entity_id)
            .ok()
            .map(|m| m.0.clone())
            .unwrap_or_else(|| "camgrn".to_string());

        let initial_alertness = v_alertness
            .get(entity_id)
            .ok()
            .map(|v| (v.level, v.peak))
            .unwrap_or((AIAlertLevel::Lowest, AIAlertLevel::Lowest));

        drop((
            v_device,
            v_camera,
            v_alert_cap,
            v_alertness,
            v_aware_delay,
            v_model_name,
        ));

        let timings = CameraTimings {
            to_two: ms_to_seconds(aware_delay.to_two),
            to_three: ms_to_seconds(aware_delay.to_three),
            two_reuse: ms_to_seconds(aware_delay.two_reuse),
            three_reuse: ms_to_seconds(aware_delay.three_reuse),
            ignore_range: ms_to_seconds(aware_delay.ignore_range),
        };

        let models = derive_models(&base_model);

        let facing_epsilon = Deg::<f32>::from(Rad(device.facing_epsilon));

        let scan_min_offset = Deg(camera.scan_angle_1);
        let scan_max_offset = Deg(camera.scan_angle_2);
        let (scan_min_offset, scan_max_offset) = if scan_min_offset.0 <= scan_max_offset.0 {
            (scan_min_offset, scan_max_offset)
        } else {
            (scan_max_offset, scan_min_offset)
        };

        let config = CameraConfig {
            device,
            camera,
            alert_cap,
            timings,
            models,
            facing_epsilon,
        };

        let mut state = CameraState {
            current_level: clamp_level(initial_alertness.0, &config.alert_cap),
            peak_level: clamp_level(initial_alertness.1, &config.alert_cap),
            visible_time: 0.0,
            hidden_time: 0.0,
            current_model: None,
            initial_yaw,
            current_offset: Deg(0.0),
            scan_min_offset,
            scan_max_offset,
            scan_direction: 1.0,
        };

        // Ensure peak never falls below the relax floor
        if level_to_u32(state.peak_level) < level_to_u32(config.alert_cap.min_relax) {
            state.peak_level = config.alert_cap.min_relax;
        }

        Some((config, state))
    }

    fn process_visibility(
        &mut self,
        entity_id: EntityId,
        visible: bool,
        delta: f32,
        config: &CameraConfig,
        effects: &mut Vec<Effect>,
    ) {
        if visible {
            self.state.visible_time += delta;
            self.state.hidden_time = 0.0;

            match self.state.current_level {
                AIAlertLevel::Lowest => {
                    if self.state.visible_time >= config.timings.to_two {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.state.visible_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Low | AIAlertLevel::Moderate => {
                    if self.state.visible_time >= config.timings.to_three {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::High,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.state.visible_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::High => {
                    self.state.visible_time = 0.0;
                }
            }
        } else {
            self.state.hidden_time += delta;
            self.state.visible_time = 0.0;

            match self.state.current_level {
                AIAlertLevel::High => {
                    if self.state.hidden_time >= config.timings.three_reuse {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.state.hidden_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Moderate => {
                    if self.state.hidden_time >= config.timings.two_reuse {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Low,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.state.hidden_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Low => {
                    if self.state.hidden_time >= config.timings.ignore_range {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Lowest,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.state.hidden_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Lowest => {
                    self.state.hidden_time = 0.0;
                }
            }
        }
    }

    fn apply_orientation(
        &mut self,
        entity_id: EntityId,
        desired_world_yaw: Option<Deg<f32>>,
        signed_horizontal: Option<Deg<f32>>,
        delta: f32,
        config: &CameraConfig,
        effects: &mut Vec<Effect>,
    ) {
        let scan_speed_deg_per_sec = (config.camera.scan_speed * 1000.0).max(0.0);
        if let Some(target_world_yaw) = desired_world_yaw {
            let target_offset = Deg(normalize_deg(target_world_yaw.0 - self.state.initial_yaw.0));
            let clamped_target = clamp_deg(
                target_offset,
                self.state.scan_min_offset,
                self.state.scan_max_offset,
            );

            let mut max_delta = scan_speed_deg_per_sec * delta;
            let delta_to_target = angle_delta(self.state.current_offset, clamped_target).abs();
            if delta_to_target <= config.facing_epsilon.0 {
                max_delta = delta_to_target;
            }

            let new_offset =
                move_towards_angle(self.state.current_offset, clamped_target, max_delta);
            if let Some(angle) = signed_horizontal {
                if angle.0.abs() > config.facing_epsilon.0 {
                    self.state.scan_direction = angle.0.signum().max(-1.0).min(1.0);
                }
            }
            self.state.current_offset = new_offset;
        } else {
            let max_delta = scan_speed_deg_per_sec * delta * self.state.scan_direction;
            let mut next = self.state.current_offset + Deg(max_delta);
            if next.0 > self.state.scan_max_offset.0 {
                next = self.state.scan_max_offset;
                self.state.scan_direction = -1.0;
            } else if next.0 < self.state.scan_min_offset.0 {
                next = self.state.scan_min_offset;
                self.state.scan_direction = 1.0;
            }
            self.state.current_offset = next;
        }

        let current_yaw = self.state.initial_yaw + self.state.current_offset;
        let yaw_difference = current_yaw.0 - self.state.initial_yaw.0;
        let base = Quaternion::from_angle_x(Deg(-90.0));
        let yaw_rotation = Quaternion::from_angle_y(Deg(-yaw_difference));
        let rotation = yaw_rotation * base;

        let joint_id = if config.device.joint_rotate >= 0 {
            config.device.joint_rotate as u32
        } else {
            1
        };

        effects.push(Effect::SetJointTransform {
            entity_id,
            joint_id,
            transform: rotation.into(),
        });
    }

    fn set_alert_level(
        &mut self,
        entity_id: EntityId,
        new_level: AIAlertLevel,
        alert_cap: &PropAIAlertCap,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let clamped_level = clamp_level(new_level, alert_cap);
        if clamped_level == self.state.current_level {
            return false;
        }

        self.state.current_level = clamped_level;

        if level_to_u32(clamped_level) > level_to_u32(self.state.peak_level) {
            self.state.peak_level = clamped_level;
        } else if level_to_u32(clamped_level) < level_to_u32(self.state.peak_level) {
            let relax_floor = alert_cap.min_relax;
            self.state.peak_level = max_level(clamped_level, relax_floor);
        }

        effects.push(Effect::SetAIProperty {
            entity_id,
            update: AIPropertyUpdate::Alertness {
                level: self.state.current_level,
                peak: self.state.peak_level,
            },
        });

        true
    }

    fn sync_model(
        &mut self,
        entity_id: EntityId,
        models: &CameraModels,
        effects: &mut Vec<Effect>,
        force: bool,
    ) {
        let target = models.model_for_level(self.state.current_level);
        if force
            || self
                .state
                .current_model
                .as_deref()
                .map(|current| current != target)
                .unwrap_or(true)
        {
            effects.push(Effect::ChangeModel {
                entity_id,
                model_name: target.to_string(),
            });
            self.state.current_model = Some(target.to_string());
        }
    }
}

impl Script for CameraAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let mut effects = Vec::new();

        if let Some((config, state)) = Self::build_config(world, entity_id) {
            self.config = Some(config);
            self.state = state;

            // Force an initial sync so the renderer and mission data match the runtime state.
            effects.push(Effect::SetAIProperty {
                entity_id,
                update: AIPropertyUpdate::Alertness {
                    level: self.state.current_level,
                    peak: self.state.peak_level,
                },
            });

            if let Some(models) = self.config.as_ref().map(|cfg| cfg.models.clone()) {
                self.sync_model(entity_id, &models, &mut effects, true);
            }

            if let Some(config) = self.config.clone() {
                let current_world_yaw = self.state.initial_yaw + self.state.current_offset;
                self.apply_orientation(
                    entity_id,
                    Some(current_world_yaw),
                    None,
                    0.0,
                    &config,
                    &mut effects,
                );
            }
        } else {
            self.config = None;
            self.state = CameraState::default();
        }

        Self::combine_effects(effects)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let mut effects = Vec::new();

        if let Some(config) = self.config.clone() {
            let delta = time.elapsed.as_secs_f32();
            let mut orientation_applied = false;
            let maybe_pose = {
                let v_pos = world.borrow::<View<PropPosition>>().unwrap();
                v_pos.get(entity_id).ok().cloned()
            };

            if let Some(pose) = maybe_pose {
                let origin = point3(pose.position.x, pose.position.y, pose.position.z);
                let forward = pose.rotation.rotate_vector(vec3(0.0, 0.0, 1.0));
                let fov = (config.camera.scan_angle_2 - config.camera.scan_angle_1)
                    .abs()
                    .max(1.0);
                let params = ai_util::VisibilityParams {
                    origin,
                    forward,
                    max_distance: 30.0,
                    horizontal_fov_deg: fov,
                };

                let visibility =
                    ai_util::camera_player_visibility(params, world, physics, entity_id);
                self.process_visibility(
                    entity_id,
                    visibility.visible,
                    delta,
                    &config,
                    &mut effects,
                );

                let desired_yaw = if visibility.visible {
                    Some(visibility.target_yaw)
                } else {
                    None
                };

                let signed_horizontal = if visibility.visible {
                    Some(visibility.signed_horizontal_angle)
                } else {
                    None
                };

                self.apply_orientation(
                    entity_id,
                    desired_yaw,
                    signed_horizontal,
                    delta,
                    &config,
                    &mut effects,
                );
                orientation_applied = true;
            } else {
                self.process_visibility(entity_id, false, delta, &config, &mut effects);
            }

            if !orientation_applied {
                self.apply_orientation(entity_id, None, None, delta, &config, &mut effects);
            }
        }

        Self::combine_effects(effects)
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

fn ms_to_seconds(value: u32) -> f32 {
    value as f32 / 1000.0
}

fn normalize_deg(mut value: f32) -> f32 {
    while value > 180.0 {
        value -= 360.0;
    }
    while value < -180.0 {
        value += 360.0;
    }
    value
}

fn angle_delta(current: Deg<f32>, target: Deg<f32>) -> f32 {
    normalize_deg(target.0 - current.0)
}

fn clamp_deg(value: Deg<f32>, min: Deg<f32>, max: Deg<f32>) -> Deg<f32> {
    let mut result = value.0;
    if result < min.0 {
        result = min.0;
    }
    if result > max.0 {
        result = max.0;
    }
    Deg(result)
}

fn move_towards_angle(current: Deg<f32>, target: Deg<f32>, max_delta: f32) -> Deg<f32> {
    if max_delta <= 0.0 {
        return current;
    }
    let delta = angle_delta(current, target);
    if delta.abs() <= max_delta {
        target
    } else {
        Deg(normalize_deg(current.0 + delta.signum() * max_delta))
    }
}

fn derive_models(base_model: &str) -> CameraModels {
    let (stem, ext) = base_model
        .rsplit_once('.')
        .map(|(stem, ext)| (stem.to_string(), Some(ext.to_string())))
        .unwrap_or_else(|| (base_model.to_string(), None));

    let lower_stem = stem.to_ascii_lowercase();
    let (yellow_stem, red_stem) = if lower_stem.ends_with("grn") {
        let prefix = &stem[..stem.len() - 3];
        (format!("{prefix}yel"), format!("{prefix}red"))
    } else if lower_stem.ends_with("green") {
        let prefix = &stem[..stem.len() - 5];
        (format!("{prefix}yellow"), format!("{prefix}red"))
    } else {
        (format!("{stem}_yel"), format!("{stem}_red"))
    };

    let rebuild = |stem_variant: String| -> String {
        if let Some(ext) = &ext {
            format!("{stem_variant}.{ext}")
        } else {
            stem_variant
        }
    };

    CameraModels {
        green: base_model.to_string(),
        yellow: rebuild(yellow_stem),
        red: rebuild(red_stem),
    }
}

fn clamp_level(level: AIAlertLevel, cap: &PropAIAlertCap) -> AIAlertLevel {
    let mut raw = level_to_u32(level);
    let min = level_to_u32(cap.min_level);
    let max = level_to_u32(cap.max_level);

    if raw < min {
        raw = min;
    }
    if raw > max {
        raw = max;
    }

    AIAlertLevel::from_u32(raw).unwrap_or(cap.max_level)
}

fn max_level(a: AIAlertLevel, b: AIAlertLevel) -> AIAlertLevel {
    if level_to_u32(a) >= level_to_u32(b) {
        a
    } else {
        b
    }
}

fn level_to_u32(level: AIAlertLevel) -> u32 {
    level.to_u32().unwrap_or(0)
}

impl CameraModels {
    fn model_for_level(&self, level: AIAlertLevel) -> &str {
        match level {
            AIAlertLevel::High => &self.red,
            AIAlertLevel::Moderate | AIAlertLevel::Low => &self.yellow,
            AIAlertLevel::Lowest => &self.green,
        }
    }
}
