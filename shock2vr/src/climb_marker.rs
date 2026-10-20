//! The floor footprint that shows where releasing every climbing hold lands
//! (see [`crate::climb_release`]): a ring around two feet, lying flat at the
//! landing and facing the player's way. Safe: a solid green ring and bright
//! feet. Long fall: a red dashed ring and dimmed feet, so colour is not the
//! only cue.

use std::rc::Rc;

use cgmath::{Deg, Matrix4, Vector3, vec3};
use engine::{
    scene::{SceneObject, basic_material},
    texture::{TextureOptions, TextureTrait, init_from_memory2},
    texture_format::{PixelFormat, RawTextureData},
};

const TEXTURE_SIZE: usize = 256;
/// Ring outer/inner radius and the faint disc inside it, in units of half the
/// texture edge.
const RING_OUTER: f32 = 0.88;
const RING_INNER: f32 = 0.78;
const DISC_ALPHA: f32 = 0.24;
/// The dashed ring: 24 degrees on in every 45.
const DASH_PERIOD: f32 = 45.0;
const DASH_START: f32 = 8.0;
const DASH_ON: f32 = 24.0;
/// Dimmed feet on the unsafe icon.
const FALL_FOOT_ALPHA: f32 = 0.47;
/// Anti-aliasing width at the shape edges, in texture pixels.
const EDGE_PIXELS: f32 = 1.5;

const SAFE_COLOR: [u8; 3] = [60, 235, 110];
const FALL_COLOR: [u8; 3] = [240, 60, 50];

/// The ring's outer edge is the standing body's footprint, slightly widened
/// so the ring reads around the feet rather than under them.
const MARKER_SIZE: f32 = crate::physics::PLAYER_STANDING_RADIUS_WORLD * 2.0 * 1.15 / RING_OUTER;
/// Lift off the floor; depth bias does the rest of the z-fight work.
const FLOOR_LIFT: f32 = 0.01;

/// Alpha coverage of an ellipse at `(x, y)`, centred at `(cx, cy)` with radii
/// `(rx, ry)` and rotated by `angle` (radians); 1 inside, soft at the edge.
fn ellipse(
    x: f32,
    y: f32,
    (cx, cy): (f32, f32),
    (rx, ry): (f32, f32),
    angle: f32,
    edge: f32,
) -> f32 {
    let (s, c) = angle.sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    let (u, v) = (dx * c + dy * s, -dx * s + dy * c);
    let d = ((u / rx).powi(2) + (v / ry).powi(2)).sqrt();
    let scale = rx.min(ry);
    ((1.0 - d) * scale / edge + 0.5).clamp(0.0, 1.0)
}

/// One toe-less foot (sole + heel) at `(cx, cy)` of length scale `s`, toes up.
fn foot(x: f32, y: f32, cx: f32, cy: f32, s: f32, angle: f32, edge: f32) -> f32 {
    // The two pads sit along the rotated foot axis below its origin.
    let along = |d: f32| (cx - d * angle.sin(), cy + d * angle.cos());
    ellipse(x, y, along(0.18 * s), (0.20 * s, 0.30 * s), angle, edge).max(ellipse(
        x,
        y,
        along(0.55 * s),
        (0.15 * s, 0.18 * s),
        angle,
        edge,
    ))
}

/// The footprint icon. Image rows run top (toes) to bottom (heels); x and y
/// below are in 0..1 of the texture edge.
pub fn footprint_texture(safe: bool) -> RawTextureData {
    let size = TEXTURE_SIZE;
    let edge = EDGE_PIXELS / size as f32;
    let color = if safe { SAFE_COLOR } else { FALL_COLOR };
    let foot_alpha = if safe { 1.0 } else { FALL_FOOT_ALPHA };
    let s = 230.0 / 512.0;
    let mut bytes = Vec::with_capacity(size * size * 4);
    for row in 0..size {
        for column in 0..size {
            let x = (column as f32 + 0.5) / size as f32;
            let y = (row as f32 + 0.5) / size as f32;
            let (dx, dy) = (x * 2.0 - 1.0, y * 2.0 - 1.0);
            let radius = (dx * dx + dy * dy).sqrt();
            let half_edge = edge * 2.0;
            let inside = ((RING_OUTER - radius) / half_edge + 0.5).clamp(0.0, 1.0);
            let ring = inside.min(((radius - RING_INNER) / half_edge + 0.5).clamp(0.0, 1.0));
            let phase = dy.atan2(dx).to_degrees().rem_euclid(DASH_PERIOD);
            let ring = if safe || (DASH_START..DASH_START + DASH_ON).contains(&phase) {
                ring
            } else {
                0.0
            };
            // Left foot, and the right mirrored across the centre and set back.
            let feet = foot(x, y, 0.34, 0.33, s, -0.12, edge).max(foot(
                1.0 - x,
                y,
                0.34,
                0.39,
                s,
                -0.12,
                edge,
            ));
            let alpha = (inside * DISC_ALPHA).max(ring).max(feet * foot_alpha);
            bytes.extend_from_slice(&[color[0], color[1], color[2], (alpha * 255.0).round() as u8]);
        }
    }
    RawTextureData {
        width: size as u32,
        height: size as u32,
        bytes,
        format: PixelFormat::RGBA,
    }
}

/// Both icons, uploaded on first use.
#[derive(Default)]
pub struct LandingMarker {
    textures: Option<[Rc<dyn TextureTrait>; 2]>,
}

impl LandingMarker {
    /// The footprint lying on the floor at `center` (x/z) and `floor_y`,
    /// toes along `yaw` (radians about +Y; 0 faces -Z).
    pub fn object(
        &mut self,
        center: Vector3<f32>,
        floor_y: f32,
        yaw: f32,
        safe: bool,
    ) -> SceneObject {
        let textures = self.textures.get_or_insert_with(|| {
            [true, false].map(|safe| {
                Rc::new(init_from_memory2(
                    footprint_texture(safe),
                    &TextureOptions {
                        wrap: false,
                        ..Default::default()
                    },
                )) as Rc<dyn TextureTrait>
            })
        });
        let texture = textures[usize::from(!safe)].clone();
        // Nearly opaque so it joins the blended pass for its per-pixel alpha;
        // full emission keeps it readable in a dark shaft.
        let mut object = SceneObject::new(
            basic_material::create_with_fixed_ambient(texture, 1.0, 0.02),
            Box::new(engine::scene::quad::create()),
        );
        object.set_transform(
            Matrix4::from_translation(vec3(center.x, floor_y + FLOOR_LIFT, center.z))
                * Matrix4::from_angle_y(cgmath::Rad(yaw))
                * Matrix4::from_angle_x(Deg(-90.0))
                * Matrix4::from_scale(MARKER_SIZE),
        );
        object.set_depth_bias(true);
        object
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(texture: &RawTextureData, x: f32, y: f32) -> u8 {
        let size = texture.width as usize;
        let (column, row) = ((x * size as f32) as usize, (y * size as f32) as usize);
        texture.bytes[(row * size + column) * 4 + 3]
    }

    /// Points on the ring at `degrees` (0 = right, clockwise in image rows).
    fn ring_point(degrees: f32) -> (f32, f32) {
        let r = (RING_OUTER + RING_INNER) / 2.0 / 2.0;
        let a = degrees.to_radians();
        (0.5 + r * a.cos(), 0.5 + r * a.sin())
    }

    #[test]
    fn the_safe_ring_is_solid_and_the_fall_ring_is_dashed() {
        let safe = footprint_texture(true);
        let fall = footprint_texture(false);
        for degrees in [2.0, 20.0, 40.0] {
            let (x, y) = ring_point(degrees);
            assert_eq!(alpha(&safe, x, y), 255, "solid at {degrees}");
        }
        let (x, y) = ring_point(20.0);
        assert_eq!(alpha(&fall, x, y), 255, "dash at 20");
        let (x, y) = ring_point(40.0);
        assert!(alpha(&fall, x, y) < 80, "gap at 40");
    }

    #[test]
    fn the_feet_are_bright_when_safe_dim_when_not_over_a_faint_disc() {
        let (safe, fall) = (footprint_texture(true), footprint_texture(false));
        // Left foot's sole, and the right's (mirrored).
        for (x, y) in [(0.34, 0.41), (0.66, 0.47)] {
            assert_eq!(alpha(&safe, x, y), 255);
            assert_eq!(alpha(&fall, x, y), (FALL_FOOT_ALPHA * 255.0).round() as u8);
        }
        // Between the feet: only the disc.
        assert_eq!(alpha(&safe, 0.5, 0.75), (DISC_ALPHA * 255.0).round() as u8);
        // Outside the ring: nothing.
        assert_eq!(alpha(&safe, 0.02, 0.02), 0);
    }
}
