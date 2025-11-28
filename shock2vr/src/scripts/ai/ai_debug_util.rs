//! Debug visualization utilities for AI entities
//!
//! Provides shared debug drawing functions for visualizing AI state
//! such as alertness levels, visibility status, and field of view.

use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, point3, vec3, vec4};
use dark::properties::AIAlertLevel;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::mission::DebugOptions;
use dark::properties::PropPosition;

use super::alertness::AlertnessState;
use crate::scripts::Effect;

/// Configuration for debug alertness visualization
pub struct AlertnessDebugConfig {
    /// Offset from entity position to base of alertness bar
    pub bar_offset: Vector3<f32>,
    /// Offset from entity position to visibility indicator start
    pub visibility_offset: Vector3<f32>,
    /// Length of visibility indicator line
    pub visibility_length: f32,
}

impl Default for AlertnessDebugConfig {
    fn default() -> Self {
        Self {
            bar_offset: vec3(0.0, 0.0, 0.0),
            visibility_offset: vec3(-0.25, 0.0, 0.0),
            visibility_length: 0.5,
        }
    }
}

impl AlertnessDebugConfig {
    /// Configuration for turrets (bar offset to side)
    pub fn turret() -> Self {
        Self {
            bar_offset: vec3(0.5, 0.0, 0.0),
            visibility_offset: vec3(-0.5, 0.5, 0.0),
            visibility_length: 0.5,
        }
    }

    /// Configuration for monsters (bar above head)
    pub fn monster() -> Self {
        Self {
            bar_offset: vec3(0.0, 1.5, 0.0),
            visibility_offset: vec3(-0.25, 1.5, 0.0),
            visibility_length: 0.5,
        }
    }

    /// Configuration for cameras (bar offset to side, lower)
    pub fn camera() -> Self {
        Self {
            bar_offset: vec3(0.5, 0.0, 0.0),
            visibility_offset: vec3(-0.5, 0.5, 0.0),
            visibility_length: 0.5,
        }
    }
}

/// Get color for alertness level
fn alertness_level_color(level: AIAlertLevel) -> cgmath::Vector4<f32> {
    match level {
        AIAlertLevel::Lowest => vec4(0.0, 0.5, 0.0, 1.0), // Dark green
        AIAlertLevel::Low => vec4(0.5, 0.5, 0.0, 1.0),    // Yellow-green
        AIAlertLevel::Moderate => vec4(1.0, 0.5, 0.0, 1.0), // Orange
        AIAlertLevel::High => vec4(1.0, 0.0, 0.0, 1.0),   // Red
    }
}

/// Get height multiplier for alertness level bar
fn alertness_level_height(level: AIAlertLevel) -> f32 {
    match level {
        AIAlertLevel::Lowest => 0.25,
        AIAlertLevel::Low => 0.5,
        AIAlertLevel::Moderate => 0.75,
        AIAlertLevel::High => 1.0,
    }
}

/// Draw debug visualization showing AI alertness state
///
/// Draws:
/// - A vertical bar whose height and color indicate alertness level
/// - A horizontal line indicating visibility (green=visible, gray=hidden)
///
/// Returns `Effect::NoEffect` if debug_ai is not enabled or entity has no position.
pub fn draw_debug_alertness(
    world: &World,
    entity_id: EntityId,
    alertness: &AlertnessState,
    is_visible: bool,
    config: &AlertnessDebugConfig,
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

        let level_color = alertness_level_color(alertness.current_level);
        let level_height = alertness_level_height(alertness.current_level);

        // Visibility indicator color
        let vis_color = if is_visible {
            vec4(0.0, 1.0, 0.0, 1.0) // Green = visible
        } else {
            vec4(0.5, 0.5, 0.5, 1.0) // Gray = not visible
        };

        // Calculate bar positions
        let bar_base = base_pos + config.bar_offset;
        let bar_top = bar_base + vec3(0.0, level_height, 0.0);

        // Calculate visibility indicator positions
        let vis_base = base_pos + config.visibility_offset;
        let vis_end = vis_base + vec3(0.0, 0.0, config.visibility_length);

        return Effect::DrawDebugLines {
            lines: vec![
                // Alertness level bar
                (
                    point3(bar_base.x, bar_base.y, bar_base.z),
                    point3(bar_top.x, bar_top.y, bar_top.z),
                    level_color,
                ),
                // Visibility indicator
                (
                    point3(vis_base.x, vis_base.y, vis_base.z),
                    point3(vis_end.x, vis_end.y, vis_end.z),
                    vis_color,
                ),
            ],
        };
    }

    Effect::NoEffect
}

/// Configuration for FOV debug visualization
pub struct FovDebugConfig {
    /// Height offset for the FOV origin point
    pub height_offset: f32,
    /// Length of the FOV lines
    pub line_length: f32,
    /// Half-angle of the FOV cone (in degrees)
    pub fov_half_angle: f32,
}

impl Default for FovDebugConfig {
    fn default() -> Self {
        Self {
            height_offset: 0.5,
            line_length: 5.0,
            fov_half_angle: 45.0,
        }
    }
}

impl FovDebugConfig {
    /// Configuration for turrets (narrower FOV)
    pub fn turret() -> Self {
        Self {
            height_offset: 0.3,
            line_length: 8.0,
            fov_half_angle: 30.0,
        }
    }

    /// Configuration for monsters (wider FOV, higher origin)
    pub fn monster() -> Self {
        Self {
            height_offset: 1.2,
            line_length: 6.0,
            fov_half_angle: 60.0,
        }
    }
}

/// Draw debug visualization showing AI field of view
///
/// Draws:
/// - A forward-facing line (green if player visible, red if not)
/// - Two side lines showing the FOV cone edges (blue)
///
/// For cameras, an optional `aim_angle` can offset the forward direction.
/// For turrets/monsters, pass the entity's current heading as `aim_angle`.
///
/// Returns `Effect::NoEffect` if debug_ai is not enabled or entity has no position.
pub fn draw_debug_fov(
    world: &World,
    entity_id: EntityId,
    heading: Deg<f32>,
    is_visible: bool,
    config: &FovDebugConfig,
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
        let origin = point3(pose.position.x, pose.position.y, pose.position.z);

        // Calculate forward direction from heading
        let orientation = pose.rotation * Quaternion::from_angle_y(-heading);
        let forward = orientation.rotate_vector(vec3(0.0, 0.0, 1.0)).normalize();

        // Calculate right vector for FOV cone
        let up = Vector3::new(0.0, 1.0, 0.0);
        let mut right = forward.cross(up);
        if right.magnitude2() < 1e-4 {
            right = Vector3::new(1.0, 0.0, 0.0);
        } else {
            right = right.normalize();
        }

        // Calculate FOV cone edges
        let fov_half_rad = config.fov_half_angle.to_radians();
        let cos_half = fov_half_rad.cos();
        let sin_half = fov_half_rad.sin();
        let rotated_left = (forward * cos_half) - (right * sin_half);
        let rotated_right = (forward * cos_half) + (right * sin_half);

        let origin_point = origin + vec3(0.0, config.height_offset, 0.0);

        // Main forward line color based on visibility
        let main_color = if is_visible {
            vec4(0.0, 1.0, 0.0, 1.0) // Green = player visible
        } else {
            vec4(1.0, 0.0, 0.0, 1.0) // Red = player not visible
        };

        // FOV cone edge color
        let edge_color = vec4(0.0, 0.5, 1.0, 1.0); // Blue

        return Effect::DrawDebugLines {
            lines: vec![
                // Forward direction
                (
                    origin_point,
                    origin_point + forward * config.line_length,
                    main_color,
                ),
                // Left FOV edge
                (
                    origin_point,
                    origin_point + rotated_left.normalize() * config.line_length,
                    edge_color,
                ),
                // Right FOV edge
                (
                    origin_point,
                    origin_point + rotated_right.normalize() * config.line_length,
                    edge_color,
                ),
            ],
        };
    }

    Effect::NoEffect
}
