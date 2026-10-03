//! Artificial turn input and movement vignette for both presentations. Physical tracking is
//! never filtered, slowed, or used as a reason to restrict the view.
use crate::ui::entry_ramp::{EntryExitRamp, RampParams};
use crate::user_settings::{TurnMode, VignetteStrength, VrSettings};
use cgmath::{Matrix4, SquareMatrix, Vector2, vec3};
use engine::scene::SceneObject;

/// Keep the tracked eye stationary while yaw rotates the rest of the rig.
/// Only horizontal origin changes are needed: artificial turns never pitch.
pub fn turn_origin_shift(
    previous: cgmath::Quaternion<f32>,
    next: cgmath::Quaternion<f32>,
    head: cgmath::Vector3<f32>,
) -> cgmath::Vector3<f32> {
    let horizontal = vec3(head.x, 0.0, head.z);
    let shift = previous * horizontal - next * horizontal;
    vec3(shift.x, 0.0, shift.z)
}

#[derive(Default)]
pub struct VrComfort {
    turn_armed: bool,
    last_turn_settings: Option<(TurnMode, f32, f32)>,
    vignette: EntryExitRamp,
}

impl VrComfort {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn disarm_turn(&mut self) {
        self.turn_armed = false;
    }

    pub fn turn_radians(&mut self, axis: f32, dt: f32, settings: VrSettings) -> f32 {
        if !axis.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return 0.0;
        }
        let signature = (settings.turning, settings.snap_angle, settings.smooth_speed);
        if self.last_turn_settings != Some(signature) {
            self.turn_armed = false;
            self.last_turn_settings = Some(signature);
        }
        let axis = axis.clamp(-1.0, 1.0);
        if axis.abs() < 0.25 {
            self.turn_armed = true;
        }
        match settings.turning {
            TurnMode::Snap if self.turn_armed && axis.abs() >= 0.7 => {
                self.turn_armed = false;
                axis.signum() * settings.snap_angle.to_radians()
            }
            TurnMode::Smooth if self.turn_armed && axis.abs() > 0.15 => {
                axis * settings.smooth_speed.to_radians() * dt
            }
            _ => 0.0,
        }
    }

    pub fn update_vignette(
        &mut self,
        movement: Vector2<f32>,
        turn_radians: f32,
        dt: f32,
        settings: VrSettings,
    ) {
        if settings.vignette == VignetteStrength::Off {
            self.vignette.snap_closed();
            return;
        }
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let moving =
            settings.vignette_movement && (movement.x.abs() > 0.15 || movement.y.abs() > 0.15);
        let turning = settings.vignette_turning
            && settings.turning == TurnMode::Smooth
            && turn_radians.abs() > 0.0001;
        // A short eased entry and a gentler release avoid a flashing border as
        // the stick crosses its dead zone. Snap turns do not trigger the mask.
        if moving || turning {
            self.vignette.open(RampParams {
                attack_secs: 0.2,
                release_secs: 0.4,
            });
        } else {
            self.vignette.close();
        }
        self.vignette.update(dt);
    }

    pub fn render(
        &self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        settings: VrSettings,
    ) -> Option<SceneObject> {
        if settings.vignette == VignetteStrength::Off || self.vignette.eased() <= 0.0 {
            return None;
        }
        let eye = view.invert()?;
        Some(crate::hit_feedback::vignette_layer_at_pose(
            crate::hit_feedback::view_extents_from_projection(projection),
            eye,
            vec3(0.0, 0.0, 0.0),
            self.vignette.eased(),
            settings.vignette.radii(),
            "vr_comfort_vignette",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, perspective, vec2};

    #[test]
    fn turning_keeps_an_offset_tracked_eye_fixed() {
        use cgmath::{InnerSpace, Quaternion, Rotation3};
        let origin = vec3(7.0, 2.0, -3.0);
        let head = vec3(0.5, 0.7, -0.2);
        let previous = Quaternion::from_angle_y(Deg(70.0));
        for angle in [30.0, -45.0, 60.0, 1.5] {
            let next = previous * Quaternion::from_angle_y(Deg(angle));
            let shifted = origin + turn_origin_shift(previous, next, head);
            assert!((origin + previous * head - (shifted + next * head)).magnitude() < 1e-5);
        }
    }

    #[test]
    fn vignette_stays_aligned_to_the_eye_at_any_head_roll() {
        use cgmath::InnerSpace;
        let settings = VrSettings::default();
        let projection = perspective(Deg(90.0), 1.5, 0.1, 100.0);
        let mut comfort = VrComfort::default();
        comfort.update_vignette(vec2(1.0, 0.0), 0.0, 0.2, settings);
        let reference = comfort
            .render(Matrix4::identity(), projection, settings)
            .unwrap()
            .get_transform();
        for roll in [30.0, 45.0, 90.0] {
            let pose = Matrix4::from_translation(vec3(1.0, 2.0, 3.0))
                * Matrix4::from_angle_y(Deg(60.0))
                * Matrix4::from_angle_x(Deg(20.0))
                * Matrix4::from_angle_z(Deg(roll));
            let view = pose.invert().unwrap();
            let camera_space = view
                * comfort
                    .render(view, projection, settings)
                    .unwrap()
                    .get_transform();
            for column in 0..4 {
                assert!((camera_space[column] - reference[column]).magnitude() < 1e-5);
            }
        }
    }

    #[test]
    fn snap_requires_neutral_and_does_not_repeat_or_reverse_while_held() {
        let mut comfort = VrComfort::default();
        let settings = VrSettings::default();
        assert_eq!(comfort.turn_radians(1.0, 0.016, settings), 0.0);
        comfort.turn_radians(0.0, 0.016, settings);
        assert_eq!(
            comfort.turn_radians(1.0, 0.016, settings),
            30.0_f32.to_radians()
        );
        for axis in [1.0, 0.5, -1.0] {
            assert_eq!(comfort.turn_radians(axis, 0.016, settings), 0.0);
        }
        comfort.turn_radians(0.0, 0.016, settings);
        assert_eq!(
            comfort.turn_radians(-1.0, 0.016, settings),
            -30.0_f32.to_radians()
        );
        comfort.reset();
        assert_eq!(comfort.turn_radians(1.0, 0.016, settings), 0.0);
    }

    #[test]
    fn smooth_turn_integrates_time_but_never_moves_a_frozen_scene() {
        let mut comfort = VrComfort::default();
        let settings = VrSettings {
            turning: TurnMode::Smooth,
            ..Default::default()
        };
        comfort.turn_radians(0.0, 0.016, settings);
        let yaw: f32 = (0..60)
            .map(|_| comfort.turn_radians(1.0, 1.0 / 60.0, settings))
            .sum();
        assert!((yaw - 90.0_f32.to_radians()).abs() < 1e-5);
        assert_eq!(comfort.turn_radians(1.0, 0.0, settings), 0.0);
    }

    #[test]
    fn vignette_tracks_only_enabled_artificial_motion_and_fades_out() {
        let mut comfort = VrComfort::default();
        let mut settings = VrSettings::default();
        comfort.update_vignette(vec2(0.0, 1.0), 0.0, 0.1, settings);
        assert_eq!(comfort.vignette.eased(), 0.5);
        comfort.update_vignette(vec2(0.0, 1.0), 0.0, 0.1, settings);
        assert_eq!(comfort.vignette.eased(), 1.0);
        for _ in 0..4 {
            comfort.update_vignette(vec2(0.0, 0.0), 0.0, 0.1, settings);
        }
        assert_eq!(comfort.vignette.eased(), 0.0);
        comfort.update_vignette(vec2(0.0, 0.0), 0.5, 0.1, settings);
        assert_eq!(comfort.vignette.eased(), 0.0, "a snap turn does not tunnel");
        settings.turning = TurnMode::Smooth;
        comfort.update_vignette(vec2(0.0, 0.0), 0.02, 0.1, settings);
        assert_eq!(comfort.vignette.eased(), 0.5);
        settings.vignette = VignetteStrength::Off;
        comfort.update_vignette(vec2(0.0, 1.0), 0.02, 0.1, settings);
        assert_eq!(comfort.vignette.eased(), 0.0);
    }

    #[test]
    fn vignette_is_a_translucent_overlay_and_preserves_projection() {
        let mut comfort = VrComfort::default();
        let settings = VrSettings::default();
        let projection = perspective(Deg(90.0), 1.0, 0.1, 100.0);
        assert!(
            comfort
                .render(Matrix4::identity(), projection, settings)
                .is_none()
        );
        comfort.update_vignette(vec2(1.0, 0.0), 0.0, 0.1, settings);
        let layer = comfort
            .render(Matrix4::identity(), projection, settings)
            .unwrap();
        assert_eq!(
            layer.render_layer(),
            engine::scene::RenderLayer::SceneOverlay
        );
        assert_eq!(layer.effective_transparency(), Some(0.5));
    }
}
