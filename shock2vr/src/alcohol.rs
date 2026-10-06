//! A game-scale alcohol vital, not a physiological BAC estimate.
//!
//! One bottle adds one unit; one unit clears every 45 seconds of gameplay.
//! More than two units starts a world vertex wave and haze, reaching full strength
//! at four. Smooth strength changes avoid a flash when drinking or sobering up.
use cgmath::{Matrix4, SquareMatrix, vec3};
use serde::{Deserialize, Serialize};
use shipyard::Unique;

const MAX_LEVEL: f32 = 6.0;
const RECOVERY_PER_SECOND: f32 = 1.0 / 45.0;
const THRESHOLD: f32 = 2.0;
const FULL_EFFECT_LEVEL: f32 = 4.0;
const MAX_OPACITY: f32 = 0.16;
// Maximum horizontal shear: about six degrees at full strength.
const MAX_SHEAR: f32 = 0.10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Unique)]
pub struct AlcoholVital {
    level: f32,
    intensity: f32,
    phase: f32,
}

impl AlcoholVital {
    pub fn level(&self) -> f32 {
        self.level
    }
    pub fn intensity(&self) -> f32 {
        self.intensity
    }

    pub(crate) fn drink(&mut self) {
        self.level = (self.level + 1.0).min(MAX_LEVEL);
    }

    pub(crate) fn update(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.level = (self.level - dt * RECOVERY_PER_SECOND).max(0.0);
        let t = ((self.level - THRESHOLD) / (FULL_EFFECT_LEVEL - THRESHOLD)).clamp(0.0, 1.0);
        let target = t * t * (3.0 - 2.0 * t);
        self.intensity += (target - self.intensity) * (1.0 - (-dt).exp());
        if self.intensity < 0.0001 && target == 0.0 {
            self.intensity = 0.0;
        }
        self.phase =
            (self.phase + dt * std::f32::consts::TAU / 6.0).rem_euclid(std::f32::consts::TAU);
    }

    /// Normalize edited save data before it reaches the shader.
    pub(crate) fn normalized(mut self) -> Self {
        fn finite(value: f32, maximum: f32) -> f32 {
            if value.is_finite() {
                value.clamp(0.0, maximum)
            } else {
                0.0
            }
        }
        self.level = finite(self.level, MAX_LEVEL);
        self.intensity = finite(self.intensity, 1.0);
        self.phase = finite(self.phase, std::f32::consts::TAU);
        self
    }

    /// Apply the same world-space field in flat and VR. Player gear and UI keep
    /// their authored positions; neither the camera nor physics is displaced.
    pub(crate) fn apply_world_wave(&self, scene: &mut [engine::scene::SceneObject], origin_y: f32) {
        use crate::util::render_source;
        use engine::scene::{RenderLayer, world_wave::WorldWave};

        let wave = WorldWave {
            offset_per_height: self.intensity
                * MAX_SHEAR
                * vec3(
                    self.phase.sin(),
                    0.35 * (2.0 * self.phase).sin(),
                    self.phase.cos(),
                ),
            origin_y,
        };
        for object in scene {
            let source = object.debug_tag().and_then(|tag| tag.source.as_deref());
            let stable = object.render_layer() != RenderLayer::World
                || matches!(
                    source,
                    Some(
                        render_source::PLAYER_HANDS
                            | render_source::GAMEPLAY_HUD
                            | render_source::DEBUG_OVERLAY
                            | render_source::DAMAGE_NUMBERS
                            | render_source::FRONTEND_POINTER
                            | render_source::USE_MODE_POINTER
                    )
                );
            object.world_wave = if stable { WorldWave::default() } else { wave };
        }
    }

    pub(crate) fn render(
        &self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
    ) -> Option<engine::scene::SceneObject> {
        if self.intensity <= 0.0 {
            return None;
        }
        let color = vec3(0.45, 0.30, 0.65);
        let opacity = self.intensity * MAX_OPACITY;
        let layer = crate::hit_feedback::vignette_layer_at_pose(
            crate::hit_feedback::view_extents_from_projection(projection),
            view.invert()?,
            color,
            opacity,
            (0.50, 0.95),
            "alcohol_overlay",
        );
        // Reuse the per-eye vignette geometry, depth and UI ordering. Both eyes
        // read the same simulation phase; rendering never advances the clock.
        layer
            .material
            .borrow_mut()
            .as_any_mut()
            .downcast_mut::<engine::scene::VignetteMaterial>()
            .expect("shared vignette geometry uses VignetteMaterial")
            .set_wave_phase(self.phase);
        Some(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::scene::RenderLayer;

    #[test]
    fn world_wave_spares_gear_and_ui_and_clears_when_sober() {
        use crate::util::render_source;
        use engine::scene::{SceneObject, color_material, quad, world_wave::WorldWave};
        let create = || {
            SceneObject::new(
                color_material::create(vec3(1.0, 1.0, 1.0)),
                Box::new(quad::create()),
            )
        };
        let mut scene = vec![create()];
        for source in [
            render_source::PLAYER_HANDS,
            render_source::GAMEPLAY_HUD,
            render_source::DEBUG_OVERLAY,
            render_source::DAMAGE_NUMBERS,
            render_source::FRONTEND_POINTER,
            render_source::USE_MODE_POINTER,
        ] {
            let mut objects = vec![create()];
            crate::util::tag_render_source(&mut objects, source);
            scene.extend(objects);
        }
        for layer in [
            RenderLayer::SceneUi,
            RenderLayer::SceneOverlay,
            RenderLayer::SystemOverlay,
        ] {
            let mut object = create();
            object.set_render_layer(layer);
            scene.push(object);
        }
        let mut vital = AlcoholVital::default();
        for _ in 0..4 {
            vital.drink();
        }
        vital.update(1.0);
        vital.apply_world_wave(&mut scene, 3.0);
        assert!(scene[0].world_wave.offset_per_height.x > 0.0);
        assert_eq!(scene[0].world_wave.origin_y, 3.0);
        assert_eq!(scene[0].duplicate().world_wave, scene[0].world_wave);
        for object in &scene[1..] {
            assert_eq!(object.world_wave, WorldWave::default());
        }
        AlcoholVital::default().apply_world_wave(&mut scene, 3.0);
        assert_eq!(scene[0].world_wave.offset_per_height, vec3(0.0, 0.0, 0.0));
    }

    #[test]
    fn repeated_drinks_cross_threshold_and_fade_smoothly() {
        let mut vital = AlcoholVital::default();
        vital.drink();
        vital.update(1.0);
        assert_eq!(vital.intensity(), 0.0);
        vital.drink();
        vital.update(1.0);
        assert_eq!(vital.intensity(), 0.0);
        vital.drink();
        assert_eq!(
            vital.intensity(),
            0.0,
            "drinking does not flash the overlay"
        );
        vital.update(0.5);
        assert!(vital.intensity() > 0.0 && vital.intensity() < 0.5);
        vital.update(300.0);
        assert_eq!(vital.level(), 0.0);
        assert_eq!(vital.intensity(), 0.0);
    }

    #[test]
    fn level_is_bounded_and_paused_time_does_not_advance_any_state() {
        let mut vital = AlcoholVital::default();
        for _ in 0..100 {
            vital.drink();
        }
        assert_eq!(vital.level(), MAX_LEVEL);
        vital.update(1.0);
        let paused = vital;
        vital.update(0.0);
        vital.update(-1.0);
        vital.update(f32::NAN);
        assert_eq!(vital, paused);
        vital.update(44.0);
        assert!((vital.level() - 5.0).abs() < 0.00001);
    }

    #[test]
    fn serialized_vital_resumes_decay_and_phase() {
        let mut vital = AlcoholVital::default();
        for _ in 0..4 {
            vital.drink();
        }
        vital.update(0.5);
        let mut restored: AlcoholVital =
            serde_json::from_str(&serde_json::to_string(&vital).unwrap()).unwrap();
        assert_eq!(restored, vital);
        restored.update(1.0);
        vital.update(1.0);
        assert_eq!(restored, vital);
        assert_eq!(
            AlcoholVital {
                level: f32::INFINITY,
                intensity: f32::NAN,
                phase: -1.0
            }
            .normalized(),
            AlcoholVital::default()
        );
    }

    #[test]
    fn overlay_is_per_eye_translucent_and_below_ui_without_changing_state() {
        let mut vital = AlcoholVital::default();
        assert!(
            vital
                .render(Matrix4::identity(), Matrix4::identity())
                .is_none()
        );
        for _ in 0..4 {
            vital.drink();
        }
        vital.update(1.0);
        let before = vital;
        let projection = cgmath::perspective(cgmath::Deg(90.0), 1.0, 0.1, 100.0);
        for eye in [-0.03, 0.03] {
            let view = Matrix4::from_translation(vec3(eye, 0.0, 0.0));
            let layer = vital.render(view, projection).unwrap();
            assert_eq!(layer.render_layer(), RenderLayer::SceneOverlay);
            assert!(!layer.depth_write);
            let transparency = layer.material.borrow().transparency().unwrap();
            assert!(transparency > 0.8 && transparency < 1.0);
            assert_eq!(vital, before);
        }
    }
}
