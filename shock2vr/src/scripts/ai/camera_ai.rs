use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, point3, vec3, vec4};
use dark::properties::{
    AIAlertLevel, PropAIAlertCap, PropAIAlertness, PropAIAwareDelay, PropAICamera, PropAIDevice,
    PropClassTag, PropModelName, PropPosition, PropSpeechVoice, PropVoiceIndex,
};
use num_traits::{FromPrimitive, ToPrimitive};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    scripts::{AIPropertyUpdate, Effect, ai::ai_util, speech_registry::SpeechVoiceRegistry},
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
    voice_index: Option<usize>,
}

#[derive(Clone)]
struct CameraTimings {
    to_two: f32,
    to_three: f32,
    two_reuse: f32,
    three_reuse: f32,
    ignore_range: f32,
}

const ALERT_ESCALATE_SECONDS: f32 = 3.0;
const ALERT_DECAY_SECONDS: f32 = 5.0;
const DEFAULT_TO_TWO_SECONDS: f32 = ALERT_ESCALATE_SECONDS;
const DEFAULT_TO_THREE_SECONDS: f32 = ALERT_ESCALATE_SECONDS;
const DEFAULT_DECAY_SECONDS: f32 = ALERT_DECAY_SECONDS;
const DEFAULT_IGNORE_SECONDS: f32 = ALERT_DECAY_SECONDS;

const CAMERA_SPEECH_LOOP_DELAY: f32 = 1.5;
const CAMERA_SPEECH_MIN_INTERVAL: f32 = 1.0;

#[derive(Clone)]
struct CameraModels {
    green: String,
    yellow: String,
    red: String,
}

struct CameraState {
    current_level: AIAlertLevel,
    peak_level: AIAlertLevel,
    current_model: Option<String>,
    visible_time: f32,
    hidden_time: f32,
    view_angle: f32,
    time_since_level_change: f32,
    time_since_last_speech: f32,
    played_at_level_line: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            current_level: AIAlertLevel::Lowest,
            peak_level: AIAlertLevel::Lowest,
            current_model: None,
            visible_time: 0.0,
            hidden_time: 0.0,
            view_angle: 0.0,
            time_since_level_change: 0.0,
            time_since_last_speech: CAMERA_SPEECH_MIN_INTERVAL,
            played_at_level_line: true,
        }
    }
}

impl CameraState {
    fn reset_for_level(&mut self, level: AIAlertLevel) {
        self.time_since_level_change = 0.0;
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
        was_visible: bool,
        effects: &mut Vec<Effect>,
    ) {
        self.state.reset_for_level(self.state.current_level);

        if config.voice_index.is_none() {
            println!("voice index is none");
            return;
        }

        let previous_rank = level_to_u32(previous_level);
        let new_rank = level_to_u32(self.state.current_level);

        if new_rank > previous_rank {
            let concept = match self.state.current_level {
                AIAlertLevel::Low => Some("tolevelone"),
                AIAlertLevel::Moderate => Some("toleveltwo"),
                AIAlertLevel::High => Some("tolevelthree"),
                AIAlertLevel::Lowest => None,
            };

            if let Some(concept) = concept {
                self.enqueue_speech(entity_id, config, concept, &[], effects);
            }
        } else {
            if previous_level == AIAlertLevel::High
                && self.state.current_level == AIAlertLevel::Moderate
                && !was_visible
            {
                self.enqueue_speech(entity_id, config, "lostcontact", &[], effects);
            }

            if self.state.current_level == AIAlertLevel::Lowest {
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

        let concept = match self.state.current_level {
            AIAlertLevel::Moderate => Some("atleveltwo"),
            AIAlertLevel::High => Some("atlevelthree"),
            _ => None,
        };

        if let Some(concept) = concept {
            if !self.state.played_at_level_line
                && self.state.time_since_level_change >= CAMERA_SPEECH_LOOP_DELAY
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

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAIAwareDelay {
                to_two: (DEFAULT_TO_TWO_SECONDS * 1000.0) as u32,
                to_three: (DEFAULT_TO_THREE_SECONDS * 1000.0) as u32,
                two_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
                three_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as u32,
                ignore_range: (DEFAULT_IGNORE_SECONDS * 1000.0) as u32,
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

        let voice_index_direct = v_voice_index
            .get(entity_id)
            .ok()
            .map(|v| v.0)
            .and_then(|idx| if idx >= 0 { Some(idx as usize) } else { None });

        let voice_label = v_voice_label
            .get(entity_id)
            .ok()
            .map(|label| label.0.clone());

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

        let mut voice_index = voice_index_direct;

        if voice_index.is_none() {
            if let Some(label) = voice_label.as_deref() {
                voice_index = lookup_voice_index_by_label(world, label);
            }
        }

        if let Some(idx) = voice_index {
            tracing::debug!("camera entity {:?} resolved voice index {}", entity_id, idx);
        } else {
            tracing::warn!(
                "camera entity {:?} missing voice index despite labels {:?}",
                entity_id,
                voice_label
            );
        }

        if voice_index.is_none() {
            voice_index = infer_voice_index_from_creature_type(world, entity_id);
        }

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
            voice_index,
        };

        let mut state = CameraState {
            current_level: clamp_level(initial_alertness.0, &config.alert_cap),
            peak_level: clamp_level(initial_alertness.1, &config.alert_cap),
            current_model: None,
            visible_time: 0.0,
            hidden_time: 0.0,
            view_angle: 0.0,
            time_since_level_change: 0.0,
            time_since_last_speech: CAMERA_SPEECH_MIN_INTERVAL,
            played_at_level_line: true,
        };

        // Ensure peak never falls below the relax floor
        if level_to_u32(state.peak_level) < level_to_u32(config.alert_cap.min_relax) {
            state.peak_level = config.alert_cap.min_relax;
        }

        state.reset_for_level(state.current_level);

        Some((config, state))
    }

    fn process_alertness(
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
                        let previous_level = self.state.current_level;
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.on_alert_level_changed(
                                entity_id,
                                config,
                                previous_level,
                                true,
                                effects,
                            );
                            self.state.visible_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Low | AIAlertLevel::Moderate => {
                    if self.state.visible_time >= config.timings.to_three {
                        let previous_level = self.state.current_level;
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::High,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.on_alert_level_changed(
                                entity_id,
                                config,
                                previous_level,
                                true,
                                effects,
                            );
                            self.state.visible_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::High => {}
            }
        } else {
            self.state.hidden_time += delta;
            self.state.visible_time = 0.0;

            match self.state.current_level {
                AIAlertLevel::High => {
                    if self.state.hidden_time >= config.timings.three_reuse {
                        let previous_level = self.state.current_level;
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Moderate,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.on_alert_level_changed(
                                entity_id,
                                config,
                                previous_level,
                                false,
                                effects,
                            );
                            self.state.hidden_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Moderate => {
                    if self.state.hidden_time >= config.timings.two_reuse {
                        let previous_level = self.state.current_level;
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Low,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.on_alert_level_changed(
                                entity_id,
                                config,
                                previous_level,
                                false,
                                effects,
                            );
                            self.state.hidden_time = 0.0;
                        }
                    }
                }
                AIAlertLevel::Low => {
                    if self.state.hidden_time >= config.timings.ignore_range {
                        let previous_level = self.state.current_level;
                        if self.set_alert_level(
                            entity_id,
                            AIAlertLevel::Lowest,
                            &config.alert_cap,
                            effects,
                        ) {
                            self.sync_model(entity_id, &config.models, effects, false);
                            self.on_alert_level_changed(
                                entity_id,
                                config,
                                previous_level,
                                false,
                                effects,
                            );
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

            self.process_alertness(entity_id, is_visible, delta, config, &mut effects);
        }

        self.state.time_since_level_change += delta;
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

            let debug_effect =
                draw_debug_camera_fov(world, entity_id, aim_angle, config, is_visible);
            if !matches!(debug_effect, Effect::NoEffect) {
                effects.push(debug_effect);
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

fn lookup_voice_index_by_label(world: &World, label: &str) -> Option<usize> {
    world
        .borrow::<UniqueView<SpeechVoiceRegistry>>()
        .ok()
        .and_then(|registry| registry.lookup(label))
}

fn infer_voice_index_from_creature_type(world: &World, entity_id: EntityId) -> Option<usize> {
    if let Ok(class_tags) = world.borrow::<View<PropClassTag>>() {
        if let Ok(tags) = class_tags.get(entity_id) {
            for (tag, value) in tags.class_tags() {
                if tag.eq_ignore_ascii_case("creaturetype") {
                    let label = format!("v{}", value);
                    drop(class_tags);
                    return lookup_voice_index_by_label(world, &label);
                }
            }
        }
        drop(class_tags);
    }
    None
}

fn draw_debug_camera_fov(
    world: &World,
    entity_id: EntityId,
    aim_angle: f32,
    config: &CameraConfig,
    is_visible: bool,
) -> Effect {
    if !cfg!(debug_assertions) {
        return Effect::NoEffect;
    }

    let v_pos = world.borrow::<View<PropPosition>>().unwrap();
    if let Ok(pose) = v_pos.get(entity_id) {
        let origin = point3(pose.position.x, pose.position.y, pose.position.z);
        let orientation = pose.rotation * Quaternion::from_angle_y(Deg(-aim_angle - 90.0));
        let forward = orientation.rotate_vector(vec3(0.0, 0.0, 1.0)).normalize();

        let up = Vector3::new(0.0, 1.0, 0.0);
        let mut right = forward.cross(up);
        if right.magnitude2() < 1e-4 {
            right = Vector3::new(1.0, 0.0, 0.0);
        } else {
            right = right.normalize();
        }

        let fov_total = config.camera.scan_angle_2 - config.camera.scan_angle_1;
        let fov_half_rad = (fov_total * 0.5).to_radians();
        let cos_half = fov_half_rad.cos();
        let sin_half = fov_half_rad.sin();
        let rotated_left = (forward * cos_half) - (right * sin_half);
        let rotated_right = (forward * cos_half) + (right * sin_half);

        let length = 5.0;
        let origin_point = origin + vec3(0.0, 0.5, 0.0);

        let main_color = if is_visible {
            vec4(0.0, 1.0, 0.0, 1.0)
        } else {
            vec4(1.0, 0.0, 0.0, 1.0)
        };

        return Effect::DrawDebugLines {
            lines: vec![
                (origin_point, origin_point + forward * length, main_color),
                (
                    origin_point,
                    origin_point + rotated_left.normalize() * length,
                    vec4(0.0, 0.5, 1.0, 1.0),
                ),
                (
                    origin_point,
                    origin_point + rotated_right.normalize() * length,
                    vec4(0.0, 0.5, 1.0, 1.0),
                ),
            ],
        };
    }

    Effect::NoEffect
}
