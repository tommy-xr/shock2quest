use cgmath::{Vector3, vec3};
use engine::scene::SceneObject;

use super::{ArcRenderConfig, ArcRenderer, TeleportHandState};

#[derive(Clone, Copy)]
pub struct TeleportVisualStyle {
    pub valid_arc_color: Vector3<f32>,
    pub invalid_arc_color: Vector3<f32>,
    pub valid_target_color: Vector3<f32>,
    pub invalid_target_color: Vector3<f32>,
    pub landing_scale: Vector3<f32>,
    pub landing_height_offset: f32,
}

impl Default for TeleportVisualStyle {
    fn default() -> Self {
        // System Shock 2 cyberpunk color scheme
        // Cyan/blue for valid states, orange/red for invalid states
        Self {
            valid_arc_color: vec3(0.0, 0.8, 1.0),       // Bright cyan
            invalid_arc_color: vec3(1.0, 0.35, 0.1),    // Orange-red
            valid_target_color: vec3(0.1, 0.9, 1.0),    // Slightly brighter cyan
            invalid_target_color: vec3(1.0, 0.4, 0.15), // Slightly brighter orange
            landing_scale: vec3(1.2, 0.02, 1.2),        // Much larger for better visibility (~3x)
            landing_height_offset: 0.02,
        }
    }
}

pub struct TeleportUI;

impl TeleportUI {
    pub fn build_visuals(
        left_hand: &TeleportHandState,
        right_hand: &TeleportHandState,
        style: &TeleportVisualStyle,
    ) -> Vec<SceneObject> {
        let render_config = ArcRenderConfig {
            landing_scale: style.landing_scale,
            landing_height_offset: style.landing_height_offset,
        };

        let mut visuals = Self::hand_visuals(left_hand, style, render_config);
        visuals.extend(Self::hand_visuals(right_hand, style, render_config));
        visuals
    }

    fn hand_visuals(
        hand_state: &TeleportHandState,
        style: &TeleportVisualStyle,
        render_config: ArcRenderConfig,
    ) -> Vec<SceneObject> {
        if !hand_state.is_active {
            return Vec::new();
        }

        let mut visuals = Vec::new();

        let arc_color = if hand_state.is_valid_target {
            style.valid_arc_color
        } else {
            style.invalid_arc_color
        };

        if let Some(trajectory) = hand_state.current_trajectory.as_ref() {
            let mut arc_particles = ArcRenderer::create_particle_arc(trajectory, arc_color);
            visuals.append(&mut arc_particles);
        }

        if let Some(target_position) = hand_state.target_position {
            let target_color = if hand_state.is_valid_target {
                style.valid_target_color
            } else {
                style.invalid_target_color
            };

            visuals.push(ArcRenderer::create_target_indicator(
                target_position,
                target_color,
                render_config,
                hand_state.animation_time,
            ));
        }

        visuals
    }
}
