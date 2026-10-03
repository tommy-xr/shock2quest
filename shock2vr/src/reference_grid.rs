//! A physical-play-space reference that travels with artificial locomotion,
//! while remaining independent of head position and orientation.
use crate::ui::entry_ramp::{EntryExitRamp, RampParams};
use crate::ui::{HOLOGRAM_TILE_TEXTURE, ImageKind, UiCanvas, UiElement};
use crate::user_settings::{ReferenceGridMode, VrSettings};
use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Vector3, vec2, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

const FADE: RampParams = RampParams {
    attack_secs: 0.25,
    release_secs: 0.5,
};
const GRID_METERS: f32 = 8.0;

pub struct ReferenceGrid {
    fade: EntryExitRamp,
    moving_secs: f32,
    pub(crate) stage_floor: Vector3<f32>,
}

impl Default for ReferenceGrid {
    fn default() -> Self {
        Self {
            fade: EntryExitRamp::default(),
            moving_secs: 0.0,
            stage_floor: vec3(0.0, 0.0, 0.0),
        }
    }
}

impl ReferenceGrid {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `displacement` is measured only across the physics movement step. It
    /// includes platform carry and falling, but excludes tracking, stance
    /// conversion, teleports, and the origin correction of a snap turn.
    pub fn update(
        &mut self,
        displacement: Vector3<f32>,
        artificial_input: bool,
        dt: f32,
        mode: ReferenceGridMode,
    ) {
        if mode == ReferenceGridMode::Off {
            self.reset();
            return;
        }
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        // Require sustained travel above the contact-jitter floor. Stick
        // intent responds immediately; a passive ride needs three 60Hz frames.
        let speed = displacement.magnitude() / dt;
        self.moving_secs = if speed.is_finite() && speed > 0.08 {
            self.moving_secs + dt
        } else {
            0.0
        };
        if mode == ReferenceGridMode::Always || artificial_input || self.moving_secs >= 0.05 {
            self.fade.open(FADE);
        } else {
            self.fade.close();
        }
        self.fade.update(dt);
    }

    pub fn render(
        &self,
        assets: &mut AssetCache,
        pawn_position: Vector3<f32>,
        pawn_rotation: Quaternion<f32>,
        settings: VrSettings,
    ) -> Vec<SceneObject> {
        if settings.reference_grid == ReferenceGridMode::Off || self.fade.is_settled_closed() {
            return Vec::new();
        }
        // Use the existing luminous, mipmapped SHODAN grid art. One quad, no
        // new shader or texture, at half-metre spacing in physical stage space.
        let mut canvas = UiCanvas::new(vec2(1.0, 1.0));
        canvas.push(UiElement::Image {
            position: vec2(0.0, 0.0),
            size: vec2(1.0, 1.0),
            texture: HOLOGRAM_TILE_TEXTURE.to_owned(),
            alpha: settings.grid_opacity * self.fade.eased(),
            kind: ImageKind::Hologram {
                tiles_x: 16,
                tiles_y: 16,
            },
        });
        let mut objects = canvas.render_world_space(
            assets,
            floor_transform(pawn_position, pawn_rotation, self.stage_floor),
            None,
            None,
            0.0,
        );
        for object in &mut objects {
            object.set_depth_write(false);
            object.set_backface_culling(None);
            object.set_render_layer(engine::scene::RenderLayer::SceneOverlay);
            object.set_debug_tag(Some(crate::util::render_source_tag(
                "comfort_reference_grid",
            )));
        }
        objects
    }
}

fn floor_transform(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    stage_floor: Vector3<f32>,
) -> Matrix4<f32> {
    let size = GRID_METERS / crate::METERS_PER_WORLD_UNIT;
    Matrix4::from_translation(position)
        * Matrix4::from(rotation)
        * Matrix4::from_translation(stage_floor)
        * Matrix4::from_angle_x(Deg(-90.0))
        * Matrix4::from_nonuniform_scale(size, size, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Rotation3, SquareMatrix};

    #[test]
    fn passive_vertical_and_horizontal_rides_activate_without_stick_input() {
        for delta in [
            vec3(0.0, 0.02, 0.0),
            vec3(0.02, 0.0, 0.0),
            vec3(0.0, -0.02, 0.0),
        ] {
            let mut grid = ReferenceGrid::default();
            for _ in 0..30 {
                grid.update(delta, false, 1.0 / 60.0, ReferenceGridMode::DuringMovement);
            }
            assert_eq!(grid.fade.eased(), 1.0);
            for _ in 0..40 {
                grid.update(
                    vec3(0.0, 0.0, 0.0),
                    false,
                    1.0 / 60.0,
                    ReferenceGridMode::DuringMovement,
                );
            }
            assert!(grid.fade.is_settled_closed());
        }
    }

    #[test]
    fn contact_jitter_and_a_single_correction_do_not_flash_the_grid() {
        let mut grid = ReferenceGrid::default();
        for _ in 0..120 {
            grid.update(
                vec3(0.0001, 0.0001, 0.0),
                false,
                1.0 / 60.0,
                ReferenceGridMode::DuringMovement,
            );
        }
        grid.update(
            vec3(0.0, 0.02, 0.0),
            false,
            1.0 / 60.0,
            ReferenceGridMode::DuringMovement,
        );
        grid.update(
            vec3(0.0, 0.0, 0.0),
            false,
            1.0 / 60.0,
            ReferenceGridMode::DuringMovement,
        );
        assert!(grid.fade.is_settled_closed());
    }

    #[test]
    fn modes_input_and_pause_reset_are_respected() {
        let mut grid = ReferenceGrid::default();
        let zero = vec3(0.0, 0.0, 0.0);
        grid.update(zero, true, 0.25, ReferenceGridMode::DuringMovement);
        assert_eq!(grid.fade.eased(), 1.0);
        grid.reset();
        assert!(grid.fade.is_settled_closed());
        grid.update(zero, false, 0.25, ReferenceGridMode::Always);
        assert_eq!(grid.fade.eased(), 1.0);
        grid.update(zero, true, 0.25, ReferenceGridMode::Off);
        assert!(grid.fade.is_settled_closed());
        grid.update(zero, true, 0.0, ReferenceGridMode::DuringMovement);
        assert!(grid.fade.is_settled_closed());
    }

    #[test]
    fn grid_stays_fixed_in_stage_space_through_game_translation_and_turns() {
        let floor = vec3(0.0, -1.2, 0.0);
        let eye =
            Matrix4::from_translation(vec3(0.3, 0.8, -0.2)) * Matrix4::from_angle_z(Deg(30.0));
        let reference = eye.invert().unwrap()
            * floor_transform(
                vec3(0.0, 0.0, 0.0),
                Quaternion::from_angle_y(Deg(0.0)),
                floor,
            );
        for (position, yaw) in [(vec3(10.0, 0.0, -20.0), 45.0), (vec3(0.0, 20.0, 0.0), 90.0)] {
            let rotation = Quaternion::from_angle_y(Deg(yaw));
            let pawn = Matrix4::from_translation(position) * Matrix4::from(rotation);
            let camera_space =
                (pawn * eye).invert().unwrap() * floor_transform(position, rotation, floor);
            for column in 0..4 {
                assert!((camera_space[column] - reference[column]).magnitude() < 1e-5);
            }
        }
        // Physical room-scale translation changes the view of the grid: it is
        // anchored to the stage, not glued to the head.
        let shifted_eye = Matrix4::from_translation(vec3(0.5, 0.0, 0.0)) * eye;
        let shifted = shifted_eye.invert().unwrap()
            * floor_transform(
                vec3(0.0, 0.0, 0.0),
                Quaternion::from_angle_y(Deg(0.0)),
                floor,
            );
        assert!((shifted.w - reference.w).magnitude() > 0.4);
    }
}
