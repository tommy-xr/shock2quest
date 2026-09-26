//! The input-recording badge: a red dot and "Recording", view-locked in the
//! upper left while a recording runs, so the player can see it is on. Built
//! once in eye space, so flat and VR present it identically.

use std::rc::Rc;

use cgmath::{Matrix4, Vector3, vec2};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{RenderLayer, SceneObject, basic_material},
    texture::{TextureOptions, TextureTrait, init_from_memory2},
    texture_format::{PixelFormat, RawTextureData},
};

use crate::ui::{HAlign, Rect, UiCanvas, VAlign, VR_COMPONENT_Z_STEP, WorldPanel};
use crate::util;

/// Distance from the eye, and the badge's offset from the gaze as fractions of
/// it: up and to the left, clear of the reticle but inside any headset's view.
const DISTANCE: f32 = 1.5;
const UP: f32 = 0.2;
const LEFT: f32 = 0.26;
/// Label height in metres at [`DISTANCE`] (~2.7 degrees).
const LABEL_HEIGHT: f32 = 0.07;
const CANVAS_SIZE: cgmath::Vector2<f32> = cgmath::Vector2::new(120.0, 24.0);
const DOT_SIZE: f32 = 0.045;
const DOT_TEXTURE_SIZE: usize = 32;

#[derive(Default)]
pub struct RecordingBadge {
    dot: Option<Rc<dyn TextureTrait>>,
}

impl RecordingBadge {
    /// The badge for an eye at `eye` looking along `forward` with `up`/`right`
    /// its view axes, all in pawn space; `pawn_to_world` maps it out.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        pawn_to_world: Matrix4<f32>,
        eye: Vector3<f32>,
        forward: Vector3<f32>,
        up: Vector3<f32>,
        right: Vector3<f32>,
    ) -> Vec<SceneObject> {
        let rotation = util::get_rotation_from_forward_vector(-forward);
        let anchor = eye + forward * DISTANCE + (up * UP - right * LEFT) * DISTANCE;

        // The label starts just right of the dot.
        let label_size = vec2(LABEL_HEIGHT * CANVAS_SIZE.x / CANVAS_SIZE.y, LABEL_HEIGHT);
        let label = WorldPanel {
            center: anchor + right * (DOT_SIZE * 0.75 + label_size.x / 2.0),
            rotation,
            size: label_size,
        };
        let mut canvas = UiCanvas::new(CANVAS_SIZE);
        canvas.text_native(
            Rect::new(0.0, 0.0, CANVAS_SIZE.x, CANVAS_SIZE.y),
            "Recording",
            "mainfont.fon",
            HAlign::Left,
            VAlign::Middle,
        );
        let mut objects = canvas.render_world_space(
            asset_cache,
            pawn_to_world * label.transform(),
            None,
            None,
            VR_COMPONENT_Z_STEP,
        );

        let texture = self
            .dot
            .get_or_insert_with(|| {
                Rc::new(init_from_memory2(
                    dot_texture(),
                    &TextureOptions {
                        wrap: false,
                        ..Default::default()
                    },
                ))
            })
            .clone();
        // Held just under opaque so the per-pixel alpha edge blends.
        let mut dot = SceneObject::new(
            basic_material::create_with_fixed_ambient(texture, 1.0, 0.02),
            Box::new(engine::scene::quad::create()),
        );
        dot.set_transform(
            pawn_to_world
                * Matrix4::from_translation(anchor)
                * Matrix4::from(rotation)
                * Matrix4::from_nonuniform_scale(DOT_SIZE, DOT_SIZE, 1.0),
        );
        objects.push(dot);

        for object in &mut objects {
            object.set_depth_write(false);
            object.set_render_layer(RenderLayer::SystemOverlay);
            object.set_debug_tag(Some(util::render_source_tag(
                util::render_source::RECORDING_BADGE,
            )));
        }
        objects
    }
}

/// A red disc with a one-pixel soft edge, on transparent.
fn dot_texture() -> RawTextureData {
    let size = DOT_TEXTURE_SIZE;
    let half = size as f32 / 2.0;
    let mut bytes = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let edge = half - 1.0 - (dx * dx + dy * dy).sqrt();
            let alpha = (edge + 0.5).clamp(0.0, 1.0);
            bytes.extend_from_slice(&[230, 30, 30, (alpha * 255.0) as u8]);
        }
    }
    RawTextureData {
        width: size as u32,
        height: size as u32,
        bytes,
        format: PixelFormat::RGBA,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(texture: &RawTextureData, x: usize, y: usize) -> u8 {
        texture.bytes[(y * texture.width as usize + x) * 4 + 3]
    }

    #[test]
    fn the_dot_is_an_opaque_red_disc_on_transparent() {
        let texture = dot_texture();
        let mid = DOT_TEXTURE_SIZE / 2;
        assert_eq!(alpha_at(&texture, mid, mid), 255);
        assert_eq!(texture.bytes[(mid * DOT_TEXTURE_SIZE + mid) * 4], 230);
        assert_eq!(alpha_at(&texture, 0, 0), 0);
        assert_eq!(alpha_at(&texture, 0, mid), 0);
    }
}
