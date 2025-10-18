use cgmath::Vector3;

use crate::{
    input_context::{InputContext, Hand},
    scripts::Effect,
    vr_config::Handedness,
};

use super::trajectory::ArcTrajectory;

/// Configuration for the teleport system
#[derive(Clone, Debug)]
pub struct TeleportConfig {
    pub enabled: bool,
    pub max_distance: f32,
    pub arc_gravity: f32,
    pub button_mapping: TeleportButton,
    pub trigger_threshold: f32,
    pub initial_velocity: f32,
    pub arc_segments: usize,
    pub ground_height: f32,
}

impl Default for TeleportConfig {
    fn default() -> Self {
        TeleportConfig {
            enabled: true,
            max_distance: 20.0,
            arc_gravity: 9.8,
            button_mapping: TeleportButton::Trigger,
            trigger_threshold: 0.5,
            initial_velocity: 12.0,
            arc_segments: 30,
            ground_height: 0.0,
        }
    }
}

/// Button mapping options for teleport activation
#[derive(Clone, Debug, PartialEq)]
pub enum TeleportButton {
    Trigger,
    AButton,
    Squeeze,
}

/// State for tracking teleport per hand
#[derive(Clone, Debug)]
pub struct TeleportHandState {
    pub is_active: bool,
    pub was_button_pressed: bool,
    pub target_position: Option<Vector3<f32>>,
    pub is_valid_target: bool,
    pub current_trajectory: Option<ArcTrajectory>,
}

impl Default for TeleportHandState {
    fn default() -> Self {
        TeleportHandState {
            is_active: false,
            was_button_pressed: false,
            target_position: None,
            is_valid_target: false,
            current_trajectory: None,
        }
    }
}

/// Main teleport system managing state for both hands
pub struct TeleportSystem {
    config: TeleportConfig,
    left_hand_state: TeleportHandState,
    right_hand_state: TeleportHandState,
}

impl TeleportSystem {
    pub fn new(config: TeleportConfig) -> Self {
        TeleportSystem {
            config,
            left_hand_state: TeleportHandState::default(),
            right_hand_state: TeleportHandState::default(),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(TeleportConfig::default())
    }

    /// Update teleport system based on input context
    pub fn update(&mut self, input_context: &InputContext) -> Vec<Effect> {
        if !self.config.enabled {
            return vec![Effect::NoEffect];
        }

        let mut effects = Vec::new();

        // Update left hand
        if let Some(effect) = Self::update_hand_static(
            &self.config,
            &input_context.left_hand,
            &mut self.left_hand_state,
            Handedness::Left
        ) {
            effects.push(effect);
        }

        // Update right hand
        if let Some(effect) = Self::update_hand_static(
            &self.config,
            &input_context.right_hand,
            &mut self.right_hand_state,
            Handedness::Right
        ) {
            effects.push(effect);
        }

        if effects.is_empty() {
            vec![Effect::NoEffect]
        } else {
            effects
        }
    }

    /// Update individual hand state and return teleport effect if triggered
    fn update_hand_static(
        config: &TeleportConfig,
        hand: &Hand,
        hand_state: &mut TeleportHandState,
        _handedness: Handedness,
    ) -> Option<Effect> {
        // Get button value based on configuration
        let button_value = match config.button_mapping {
            TeleportButton::Trigger => hand.trigger_value,
            TeleportButton::AButton => hand.a_value,
            TeleportButton::Squeeze => hand.squeeze_value,
        };

        let is_button_pressed = button_value >= config.trigger_threshold;

        // Check for button press/release transitions
        let button_just_pressed = is_button_pressed && !hand_state.was_button_pressed;
        let button_just_released = !is_button_pressed && hand_state.was_button_pressed;

        // Update button state tracking
        hand_state.was_button_pressed = is_button_pressed;

        // Handle teleport activation
        if button_just_pressed {
            hand_state.is_active = true;
            hand_state.target_position = None;
            hand_state.is_valid_target = false;
            hand_state.current_trajectory = None;
        }

        // Handle teleport execution on button release
        if button_just_released && hand_state.is_active {
            hand_state.is_active = false;

            // Execute teleport if we have a valid target
            if let Some(target_pos) = hand_state.target_position {
                if hand_state.is_valid_target {
                    hand_state.target_position = None;
                    hand_state.is_valid_target = false;

                    return Some(Effect::SetPlayerPosition {
                        position: target_pos,
                        is_teleport: true,
                    });
                }
            }

            // Clear state if no valid teleport
            hand_state.target_position = None;
            hand_state.is_valid_target = false;
            hand_state.current_trajectory = None;
        }

        // Update arc trajectory and target while active
        if hand_state.is_active && is_button_pressed {
            Self::update_teleport_trajectory_static(config, hand, hand_state);
        }

        None
    }

    /// Update trajectory calculation and target validation
    fn update_teleport_trajectory_static(config: &TeleportConfig, hand: &Hand, hand_state: &mut TeleportHandState) {
        // Phase 2: Calculate proper parabolic arc trajectory

        // Calculate forward direction from hand rotation
        let forward = hand.rotation * Vector3::new(0.0, 0.0, -1.0);

        // Calculate arc trajectory using physics
        let trajectory = ArcTrajectory::calculate(
            hand.position,
            forward,
            config.initial_velocity,
            config.arc_gravity,
            config.max_distance,
            config.arc_segments,
            config.ground_height,
        );

        // Update hand state with trajectory results
        hand_state.current_trajectory = Some(trajectory.clone());
        hand_state.target_position = trajectory.landing_position;
        hand_state.is_valid_target = trajectory.is_valid;
    }

    /// Get current teleport state for rendering/UI
    pub fn get_left_hand_state(&self) -> &TeleportHandState {
        &self.left_hand_state
    }

    pub fn get_right_hand_state(&self) -> &TeleportHandState {
        &self.right_hand_state
    }

    /// Get current configuration
    pub fn get_config(&self) -> &TeleportConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: TeleportConfig) {
        self.config = config;
    }
}