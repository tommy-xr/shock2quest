use cgmath::{Deg, Quaternion, Rotation3};
use dark::properties::{
    AIAlertLevel, PropAIAlertCap, PropAIAlertness, PropAIAwareDelay, PropAICamera, PropAIDevice,
    PropModelName,
};
use num_traits::{FromPrimitive, ToPrimitive};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{AIPropertyUpdate, Effect},
    time::Time,
};

use super::{MessagePayload, Script};

#[allow(dead_code)]
#[derive(Clone)]
struct CameraConfig {
    device: PropAIDevice,
    camera: PropAICamera,
    alert_cap: PropAIAlertCap,
    timings: CameraTimings,
    models: CameraModels,
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
    time_in_state: f32,
    descending: bool,
    current_model: Option<String>,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            current_level: AIAlertLevel::Lowest,
            peak_level: AIAlertLevel::Lowest,
            time_in_state: 0.0,
            descending: false,
            current_model: None,
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

        let config = CameraConfig {
            device,
            camera,
            alert_cap,
            timings,
            models,
        };

        let mut state = CameraState {
            current_level: clamp_level(initial_alertness.0, &config.alert_cap),
            peak_level: clamp_level(initial_alertness.1, &config.alert_cap),
            time_in_state: 0.0,
            descending: initial_alertness.0 != AIAlertLevel::Lowest,
            current_model: None,
        };

        // Ensure peak never falls below the relax floor
        if level_to_u32(state.peak_level) < level_to_u32(config.alert_cap.min_relax) {
            state.peak_level = config.alert_cap.min_relax;
        }

        Some((config, state))
    }

    fn advance_alertness(
        &mut self,
        delta: f32,
        entity_id: EntityId,
        alert_cap: &PropAIAlertCap,
        timings: &CameraTimings,
        models: &CameraModels,
        effects: &mut Vec<Effect>,
    ) {
        self.state.time_in_state += delta;

        if self.state.descending {
            match self.state.current_level {
                AIAlertLevel::High => {
                    if self.state.time_in_state >= timings.three_reuse {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, models, effects, false);
                            self.state.descending = true;
                        }
                    }
                }
                AIAlertLevel::Moderate => {
                    if self.state.time_in_state >= timings.two_reuse {
                        if self.set_alert_level(entity_id, AIAlertLevel::Low, alert_cap, effects) {
                            self.sync_model(entity_id, models, effects, false);
                        }
                    }
                }
                AIAlertLevel::Low => {
                    if self.state.time_in_state >= timings.ignore_range {
                        if self.set_alert_level(entity_id, AIAlertLevel::Lowest, alert_cap, effects)
                        {
                            self.sync_model(entity_id, models, effects, false);
                            self.state.descending = false;
                        }
                    }
                }
                AIAlertLevel::Lowest => {
                    self.state.descending = false;
                }
            }
        } else {
            match self.state.current_level {
                AIAlertLevel::Lowest => {
                    if self.state.time_in_state >= timings.to_two {
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, models, effects, false);
                        }
                    }
                }
                AIAlertLevel::Moderate => {
                    if self.state.time_in_state >= timings.to_three {
                        if self.set_alert_level(entity_id, AIAlertLevel::High, alert_cap, effects) {
                            self.sync_model(entity_id, models, effects, false);
                            self.state.descending = true;
                        }
                    }
                }
                AIAlertLevel::Low => {
                    if self.state.time_in_state >= timings.to_three {
                        if self.set_alert_level(entity_id, AIAlertLevel::High, alert_cap, effects) {
                            self.sync_model(entity_id, models, effects, false);
                            self.state.descending = true;
                        }
                    }
                }
                AIAlertLevel::High => {
                    // Already at peak; wait for descend logic
                }
            }
        }
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
        self.state.time_in_state = 0.0;

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
        } else {
            self.config = None;
            self.state = CameraState::default();
        }

        Self::combine_effects(effects)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let mut effects = Vec::new();

        if let Some(config) = &self.config {
            let delta = time.elapsed.as_secs_f32();
            let alert_cap = config.alert_cap.clone();
            let timings = config.timings.clone();
            let models = config.models.clone();
            self.advance_alertness(
                delta,
                entity_id,
                &alert_cap,
                &timings,
                &models,
                &mut effects,
            );
        }

        let quat = Quaternion::from_angle_x(Deg(time.total.as_secs_f32().sin() * 90.0));
        effects.push(Effect::SetJointTransform {
            entity_id,
            joint_id: 1,
            transform: quat.into(),
        });

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
