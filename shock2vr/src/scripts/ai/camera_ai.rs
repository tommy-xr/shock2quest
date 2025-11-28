use cgmath::{Deg, Quaternion, Rotation3};
use dark::properties::{
    AIAlertLevel, PropAIAlertCap, PropAIAlertness, PropAIAwareDelay, PropAICamera, PropAIDevice,
    PropModelName, PropPosition, PropSpeechVoice, PropVoiceIndex,
};
use num_traits::ToPrimitive;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    scripts::{Effect, ai::ai_util, speech_util},
    time::Time,
};

use super::{
    MessagePayload, Script,
    ai_debug_util::{self, AlertnessDebugConfig, FovDebugConfig},
    alertness::{self, AlertnessState, AlertnessTimings},
};

#[allow(dead_code)]
#[derive(Clone)]
struct CameraConfig {
    device: PropAIDevice,
    camera: PropAICamera,
    alert_cap: PropAIAlertCap,
    timings: AlertnessTimings,
    models: CameraModels,
    voice_index: Option<usize>,
}

const ALERT_ESCALATE_SECONDS: f32 = 3.0;
const ALERT_DECAY_SECONDS: f32 = 5.0;

const CAMERA_SPEECH_LOOP_DELAY: f32 = 1.5;
const CAMERA_SPEECH_MIN_INTERVAL: f32 = 1.0;

#[derive(Clone)]
struct CameraModels {
    green: String,
    yellow: String,
    red: String,
}

/// Camera-specific state that wraps the shared AlertnessState
struct CameraState {
    /// Shared alertness state (current/peak levels, visibility timers)
    alertness: AlertnessState,
    /// Current model being displayed
    current_model: Option<String>,
    /// Current view angle for joint animation
    view_angle: f32,
    /// Time since last speech was played
    time_since_last_speech: f32,
    /// Whether we've played the "at level" sustain line
    played_at_level_line: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            alertness: AlertnessState::default(),
            current_model: None,
            view_angle: 0.0,
            time_since_last_speech: CAMERA_SPEECH_MIN_INTERVAL,
            played_at_level_line: true,
        }
    }
}

impl CameraState {
    fn reset_for_level(&mut self, level: AIAlertLevel) {
        self.played_at_level_line = !matches!(level, AIAlertLevel::Moderate | AIAlertLevel::High);
    }

    fn record_speech(&mut self) {
        self.time_since_last_speech = 0.0;
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

    fn enqueue_speech(
        &mut self,
        entity_id: EntityId,
        config: &CameraConfig,
        concept: &str,
        tags: &[(String, String)],
        effects: &mut Vec<Effect>,
    ) -> bool {
        let voice_index = match config.voice_index {
            Some(idx) => idx,
            None => return false,
        };

        let concept_key = concept.to_ascii_lowercase();
        let prepared_tags = tags
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_ascii_lowercase()))
            .collect::<Vec<_>>();

        effects.push(Effect::PlaySpeech {
            entity_id,
            voice_index,
            concept: concept_key,
            tags: prepared_tags,
        });
        self.state.record_speech();
        true
    }

    fn on_alert_level_changed(
        &mut self,
        entity_id: EntityId,
        config: &CameraConfig,
        previous_level: AIAlertLevel,
        new_level: AIAlertLevel,
        was_visible: bool,
        effects: &mut Vec<Effect>,
    ) {
        self.state.reset_for_level(new_level);

        if config.voice_index.is_none() {
            tracing::debug!("camera {:?}: voice index is none", entity_id);
            return;
        }

        let previous_rank = level_to_u32(previous_level);
        let new_rank = level_to_u32(new_level);

        if new_rank > previous_rank {
            // Escalating
            let concept = match new_level {
                AIAlertLevel::Low => Some("tolevelone"),
                AIAlertLevel::Moderate => Some("toleveltwo"),
                AIAlertLevel::High => Some("tolevelthree"),
                AIAlertLevel::Lowest => None,
            };

            if let Some(concept) = concept {
                self.enqueue_speech(entity_id, config, concept, &[], effects);
            }
        } else {
            // Decaying
            if previous_level == AIAlertLevel::High
                && new_level == AIAlertLevel::Moderate
                && !was_visible
            {
                self.enqueue_speech(entity_id, config, "lostcontact", &[], effects);
            }

            if new_level == AIAlertLevel::Lowest {
                self.enqueue_speech(entity_id, config, "backtozero", &[], effects);
            }
        }
    }

    fn maybe_play_level_sustain(
        &mut self,
        entity_id: EntityId,
        config: &CameraConfig,
        effects: &mut Vec<Effect>,
    ) {
        if config.voice_index.is_none() {
            return;
        }

        let concept = match self.state.alertness.current_level {
            AIAlertLevel::Moderate => Some("atleveltwo"),
            AIAlertLevel::High => Some("atlevelthree"),
            _ => None,
        };

        if let Some(concept) = concept {
            if !self.state.played_at_level_line
                && self.state.alertness.time_since_level_change >= CAMERA_SPEECH_LOOP_DELAY
                && self.state.time_since_last_speech >= CAMERA_SPEECH_MIN_INTERVAL
            {
                if self.enqueue_speech(entity_id, config, concept, &[], effects) {
                    self.state.played_at_level_line = true;
                }
            }
        }
    }

    fn build_config(world: &World, entity_id: EntityId) -> Option<(CameraConfig, CameraState)> {
        let (
            v_device,
            v_camera,
            v_alert_cap,
            v_alertness,
            v_aware_delay,
            v_model_name,
            v_voice_index,
            v_voice_label,
        ): (
            View<PropAIDevice>,
            View<PropAICamera>,
            View<PropAIAlertCap>,
            View<PropAIAlertness>,
            View<PropAIAwareDelay>,
            View<PropModelName>,
            View<PropVoiceIndex>,
            View<PropSpeechVoice>,
        ) = world
            .borrow::<(
                View<PropAIDevice>,
                View<PropAICamera>,
                View<PropAIAlertCap>,
                View<PropAIAlertness>,
                View<PropAIAwareDelay>,
                View<PropModelName>,
                View<PropVoiceIndex>,
                View<PropSpeechVoice>,
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
                scan_angle_1: -45.0,
                scan_angle_2: 45.0,
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

        // Build default aware delay using the same constants as before
        let default_aware_delay = PropAIAwareDelay {
            to_two: (ALERT_ESCALATE_SECONDS * 1000.0) as u32,
            to_three: (ALERT_ESCALATE_SECONDS * 1000.0) as u32,
            two_reuse: (ALERT_DECAY_SECONDS * 1000.0) as u32,
            three_reuse: (ALERT_DECAY_SECONDS * 1000.0) as u32,
            ignore_range: (ALERT_DECAY_SECONDS * 1000.0) as u32,
        };

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(default_aware_delay);

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
            v_voice_index,
            v_voice_label,
        ));

        // Use helper to resolve voice index
        let voice_index = speech_util::resolve_entity_voice_index(world, entity_id);
        if voice_index.is_none() {
            tracing::warn!(
                "camera entity {:?} has no resolvable voice index",
                entity_id
            );
        }

        // Use the shared AlertnessTimings
        let timings = AlertnessTimings::from_aware_delay(&aware_delay);
        let models = derive_models(&base_model);

        let config = CameraConfig {
            device,
            camera,
            alert_cap: alert_cap.clone(),
            timings,
            models,
            voice_index,
        };

        // Initialize alertness state with clamped levels
        let initial_level = alertness::clamp_level(initial_alertness.0, &alert_cap);
        let mut initial_peak = alertness::clamp_level(initial_alertness.1, &alert_cap);

        // Ensure peak never falls below the relax floor
        if level_to_u32(initial_peak) < level_to_u32(alert_cap.min_relax) {
            initial_peak = alert_cap.min_relax;
        }

        let mut alertness_state = AlertnessState::new(initial_level);
        alertness_state.peak_level = initial_peak;

        let mut state = CameraState {
            alertness: alertness_state,
            current_model: None,
            view_angle: 0.0,
            time_since_last_speech: CAMERA_SPEECH_MIN_INTERVAL,
            played_at_level_line: true,
        };

        state.reset_for_level(state.alertness.current_level);

        Some((config, state))
    }

    fn sync_model(
        &mut self,
        entity_id: EntityId,
        models: &CameraModels,
        effects: &mut Vec<Effect>,
        force: bool,
    ) {
        let target = models.model_for_level(self.state.alertness.current_level);
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
            effects.push(alertness::sync_alertness_effect(
                entity_id,
                &self.state.alertness,
            ));

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
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let mut effects = Vec::new();
        let delta = time.elapsed.as_secs_f32();

        let mut target_angle = time.total.as_secs_f32().sin() * 90.0;
        let mut max_delta: Option<f32> = None;
        let mut is_visible = false;
        let config_clone = self.config.clone();

        if let Some(config) = config_clone.as_ref() {
            is_visible = ai_util::is_player_visible(entity_id, world, physics);
            if is_visible {
                let v_pos = world.borrow::<View<PropPosition>>().unwrap();
                if let Ok(pose) = v_pos.get(entity_id) {
                    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                    let target_yaw = ai_util::yaw_between_vectors(pose.position, u_player.pos);
                    drop(u_player);

                    let current_yaw = ai_util::current_yaw(entity_id, world);
                    target_angle = normalize_deg(current_yaw.0 - target_yaw.0 - 90.0);
                }
            }

            let speed_deg_per_sec = (config.camera.scan_speed * 1000.0).max(1.0);
            max_delta = Some(speed_deg_per_sec * delta);

            // Use the shared alertness update logic
            if let Some((old_level, new_level)) = alertness::process_alertness_update(
                &mut self.state.alertness,
                is_visible,
                delta,
                &config.timings,
                &config.alert_cap,
            ) {
                // Level changed - sync model and play speech
                self.sync_model(entity_id, &config.models, &mut effects, false);
                self.on_alert_level_changed(
                    entity_id,
                    config,
                    old_level,
                    new_level,
                    is_visible,
                    &mut effects,
                );
                // Sync alertness to ECS
                effects.push(alertness::sync_alertness_effect(
                    entity_id,
                    &self.state.alertness,
                ));
            }
        }

        self.state.time_since_last_speech += delta;

        let aim_angle = if let Some(delta_limit) = max_delta {
            self.state.view_angle =
                move_towards_angle(self.state.view_angle, target_angle, delta_limit);
            self.state.view_angle
        } else {
            target_angle
        };

        let quat = Quaternion::from_angle_x(Deg(aim_angle));
        effects.push(Effect::SetJointTransform {
            entity_id,
            joint_id: 1,
            transform: quat.into(),
        });

        if let Some(config) = config_clone.as_ref() {
            self.maybe_play_level_sustain(entity_id, config, &mut effects);

            // FOV debug visualization (camera uses aim_angle for custom orientation)
            let fov_config =
                FovDebugConfig::camera(config.camera.scan_angle_1, config.camera.scan_angle_2);
            let fov_debug_effect = ai_debug_util::draw_debug_fov(
                world,
                entity_id,
                Deg(aim_angle + 90.0), // Camera-specific offset
                is_visible,
                &fov_config,
            );
            if !matches!(fov_debug_effect, Effect::NoEffect) {
                effects.push(fov_debug_effect);
            }

            // Alertness bar debug visualization (shared with turrets/monsters)
            let alertness_debug_effect = ai_debug_util::draw_debug_alertness(
                world,
                entity_id,
                &self.state.alertness,
                is_visible,
                &AlertnessDebugConfig::camera(),
            );
            if !matches!(alertness_debug_effect, Effect::NoEffect) {
                effects.push(alertness_debug_effect);
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

fn normalize_deg(mut angle: f32) -> f32 {
    while angle > 180.0 {
        angle -= 360.0;
    }
    while angle < -180.0 {
        angle += 360.0;
    }
    angle
}

fn move_towards_angle(current: f32, target: f32, max_delta: f32) -> f32 {
    if max_delta <= 0.0 {
        return current;
    }
    let delta = normalize_deg(target - current);
    if delta.abs() <= max_delta {
        target
    } else {
        normalize_deg(current + delta.signum() * max_delta)
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
