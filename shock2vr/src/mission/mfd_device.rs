//! The handheld MFD device: the personal card's VR body, a phone-sized slab
//! whose face presents windows of the shared use-mode canvas.
//!
//! The face is its own pixel surface ([`FACE_PX`]). The MFD slot is shown on
//! the screen and a strip of the bottom bar (utilities, nanites, modules, log,
//! MFD) beneath it, each through a [`CanvasViewport`]; the canvas layout itself
//! is never re-decided here (AGENTS.md section 3).
use crate::input_context::InputContext;
use crate::ui::canvas_viewport::CanvasViewport;
use crate::ui::{FrontendPointerPass, Rect, WorldPanel};
use cgmath::EuclideanSpace;
use cgmath::{
    InnerSpace, Matrix3, Matrix4, Quaternion, SquareMatrix, Vector2, Vector3, Zero, vec2, vec3,
    vec4,
};
use engine::scene::{SceneObject, VertexPosition};
use std::{cell::OnceCell, rc::Rc};

/// Face size in metres (a large phone) and its pixel surface.
const FACE_M: Vector2<f32> = Vector2 { x: 0.09, y: 0.16 };
const THICKNESS_M: f32 = 0.012;
const CORNER_M: f32 = 0.010;
pub const FACE_PX: Vector2<f32> = Vector2 { x: 180.0, y: 320.0 };

/// Screen and bottom-bar areas on the face, in face pixels. The bar is the
/// AMMOFULL backdrop (260x64) at the face's width.
const SCREEN: Rect = Rect::new(6.0, 8.0, 168.0, 262.0);
const BAR: Rect = Rect::new(6.0, 274.0, 168.0, 168.0 * 64.0 / 260.0);
pub const BAR_ART: &str = "AMMOFULL.PCX";
/// Glass behind both areas, so empty canvas regions read as a dark display.
const GLASS: Rect = Rect::new(4.0, 6.0, 172.0, 308.0);

/// AMMOFULL's two square wells and its long recess, in art pixels.
const ART_WELLS: [Rect; 2] = [
    Rect::new(5.0, 20.0, 33.0, 33.0),
    Rect::new(44.0, 20.0, 33.0, 33.0),
];
const ART_RECESS: Rect = Rect::new(117.0, 14.0, 133.0, 46.0);

/// Shared-canvas readouts shown in the wells: nanites, cyber modules (icon
/// and count, `hud::readouts::emit_use_mode`).
const WELL_SOURCES: [Rect; 2] = [
    Rect::new(185.0, 434.0, 36.0, 34.0),
    Rect::new(224.0, 434.0, 36.0, 34.0),
];
/// Shared-canvas buttons shown in the recess, left to right: RES, ? over
/// MAP (`mission::mfd_utilities`), LOG (`hud::readouts`), MFD.
const BUTTON_SOURCES: [Rect; 4] = [
    Rect::new(117.0, 431.0, 32.0, 40.0),
    Rect::new(150.0, 431.0, 32.0, 40.0),
    Rect::new(383.0, 432.0, 38.0, 36.0),
    Rect::new(460.0, 430.0, 32.0, 40.0),
];

/// An AMMOFULL art rect placed on the face's bar.
fn art_to_face(r: Rect) -> Rect {
    let s = BAR.w / 260.0;
    Rect::new(BAR.x + r.x * s, BAR.y + r.y * s, r.w * s, r.h * s)
}

/// Held like a phone: resting against the palm with the face looking out of
/// it (controller +X for the left hand, -X for the right), its top toward the
/// fingers (-Z). `hand` is the slot index, 0 = left.
fn in_hand(hand: usize) -> Matrix4<f32> {
    let side = if hand == 0 { 1.0 } else { -1.0 };
    // Columns: device X, Y (top), Z (face normal) in the controller frame.
    let basis = Matrix4::from_cols(
        vec4(0.0, -side, 0.0, 0.0),
        vec4(0.0, 0.0, -1.0, 0.0),
        vec4(side, 0.0, 0.0, 0.0),
        vec4(0.0, 0.0, 0.0, 1.0),
    );
    Matrix4::from_translation(vec3(side * 0.065, 0.0, -0.05) / crate::METERS_PER_WORLD_UNIT) * basis
}

/// Device transform (world units) for a held hand pose.
pub fn held_transform(
    position: Vector3<f32>,
    orientation: Quaternion<f32>,
    hand: usize,
) -> Matrix4<f32> {
    Matrix4::from_translation(position) * Matrix4::from(orientation) * in_hand(hand)
}

/// The windows the face shows: the active MFD (if any) fitted to the screen,
/// the nanite and module readouts in the bar's wells, and the utility buttons
/// in its recess.
pub fn viewports(screen_source: Option<Rect>) -> Vec<CanvasViewport> {
    let mut out: Vec<_> = screen_source
        .map(|src| CanvasViewport::fit(src, SCREEN))
        .into_iter()
        .collect();
    for (src, well) in WELL_SOURCES.into_iter().zip(ART_WELLS) {
        out.push(CanvasViewport::fit(src, art_to_face(well)));
    }
    // The buttons share one scale, side by side, centered in the recess.
    let recess = art_to_face(ART_RECESS);
    let total_w: f32 = BUTTON_SOURCES.iter().map(|r| r.w).sum();
    let max_h = BUTTON_SOURCES.iter().map(|r| r.h).fold(0.0, f32::max);
    let scale = (recess.w / total_w).min(recess.h / max_h);
    let mut x = recess.x + (recess.w - total_w * scale) / 2.0;
    for src in BUTTON_SOURCES {
        let h = src.h * scale;
        let dst = Rect::new(x, recess.y + (recess.h - h) / 2.0, src.w * scale, h);
        out.push(CanvasViewport { src, dst });
        x += dst.w;
    }
    out
}

/// The face as a world panel: centered on the front surface, +Z out of the
/// screen, sized to [`FACE_M`] in world units.
pub fn face_panel(device: Matrix4<f32>) -> Option<WorldPanel> {
    let basis = Matrix3::from_cols(
        device.x.truncate().normalize(),
        device.y.truncate().normalize(),
        device.z.truncate().normalize(),
    );
    if !basis.determinant().is_finite() || basis.determinant().abs() < 0.5 {
        return None;
    }
    let center = device
        * vec4(
            0.0,
            0.0,
            (THICKNESS_M / 2.0 + 0.0005) / crate::METERS_PER_WORLD_UNIT,
            1.0,
        );
    Some(WorldPanel {
        center: center.truncate(),
        rotation: Quaternion::from(basis).normalize(),
        size: FACE_M / crate::METERS_PER_WORLD_UNIT,
    })
}

/// The pointer input for the face, in world space: each hand's controller
/// pose placed through the pawn. The holding hand is removed - its ray starts
/// under the screen and its trigger must never click it.
pub fn pointer_input(
    input: &InputContext,
    pawn: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    holding: usize,
) -> InputContext {
    let mut world = input.clone();
    for (i, hand) in [&mut world.left_hand, &mut world.right_hand]
        .into_iter()
        .enumerate()
    {
        if i == holding {
            // Zero quaternion = untracked: no ray (vr-ui-design rule 7).
            hand.rotation = Quaternion::zero();
            hand.trigger_value = 0.0;
            hand.squeeze_value = 0.0;
        } else {
            hand.position =
                crate::virtual_hand::hand_world_position(pawn, pawn_rotation, hand.position);
            hand.rotation = pawn_rotation * hand.rotation;
        }
    }
    world
}

/// A face-pixel pass re-expressed in shared-canvas pixels, for the host and
/// the per-hand arbitration. A hit on the bezel or glass outside every window
/// is not on the canvas.
pub fn to_canvas_pass(
    pass: &FrontendPointerPass,
    viewports: &[CanvasViewport],
) -> FrontendPointerPass {
    pass.remap_hits(|p| crate::ui::canvas_viewport::target_to_canvas(viewports, p))
}

/// A cached mesh shared by every frame's scene objects. `Mesh` frees its GL
/// buffers on drop, so a per-frame clone would delete the cached geometry.
struct SharedMesh(Rc<engine::scene::mesh::Mesh>);

impl engine::scene::Geometry for SharedMesh {
    fn draw(&self) {
        self.0.draw();
    }
}

thread_local! {
    static BODY: OnceCell<[Rc<engine::scene::mesh::Mesh>; 3]> = const { OnceCell::new() };
}

/// The slab: front, back and rim in three flat tones (the colour material is
/// unlit, so tone is what separates the faces), plus the dark glass.
pub fn render_body(device: Matrix4<f32>) -> Vec<SceneObject> {
    let meshes = BODY.with(|cell| cell.get_or_init(build_body).clone());
    let to_world = device * Matrix4::from_scale(1.0 / crate::METERS_PER_WORLD_UNIT);
    let [front, back, rim] = meshes;
    let mut objects: Vec<_> = [
        (front, vec3(0.13, 0.14, 0.16)),
        (back, vec3(0.08, 0.08, 0.09)),
        (rim, vec3(0.30, 0.32, 0.35)),
    ]
    .into_iter()
    .map(|(mesh, color)| {
        let mut object = SceneObject::new(
            engine::scene::color_material::create(color),
            Box::new(SharedMesh(mesh)),
        );
        object.set_transform(to_world);
        object
    })
    .collect();
    let mut glass = SceneObject::new(
        engine::scene::color_material::create(vec3(0.01, 0.03, 0.035)),
        Box::new(engine::scene::quad::create()),
    );
    glass.set_transform(to_world * face_rect_transform(GLASS, 0.0002));
    objects.push(glass);
    objects
}

/// The bar backdrop, as device chrome under the canvas windows.
pub fn bar_chrome() -> crate::ui::UiCanvas {
    let mut canvas = crate::ui::UiCanvas::new(FACE_PX);
    canvas.image(BAR, BAR_ART);
    canvas
}

/// Hologram of the scanned object: its model shrunk to [`HOLOGRAM_M`],
/// translucent, spinning about world up a hand's width above the device's
/// top edge.
const HOLOGRAM_M: f32 = 0.09;
const HOLOGRAM_LIFT_M: f32 = 0.07;
const HOLOGRAM_TRANSPARENCY: f32 = 0.4;
const HOLOGRAM_GLOW_TRANSPARENCY: f32 = 0.7;

pub fn render_hologram(
    model: &dark::model::Model,
    device: Matrix4<f32>,
    spin: f32,
) -> Vec<SceneObject> {
    // Jointed object models report their extent only through object bounds.
    let Some(bounds) = model.bounding_box().or_else(|| model.object_model_bounds()) else {
        return Vec::new();
    };
    let (min, max) = (bounds.min.to_vec(), bounds.max.to_vec());
    let extent = (max - min)
        .x
        .max((max - min).y)
        .max((max - min).z)
        .max(0.001);
    let top = device * vec4(0.0, FACE_M.y / 2.0 / crate::METERS_PER_WORLD_UNIT, 0.0, 1.0);
    let center = top.truncate()
        + vec3(
            0.0,
            (HOLOGRAM_LIFT_M + HOLOGRAM_M / 2.0) / crate::METERS_PER_WORLD_UNIT,
            0.0,
        );
    let placement = Matrix4::from_translation(center)
        * Matrix4::from_angle_y(cgmath::Rad(spin))
        * Matrix4::from_scale(HOLOGRAM_M / crate::METERS_PER_WORLD_UNIT / extent)
        * Matrix4::from_translation(-(min + max) * 0.5);
    // Each piece twice: its own texture, translucent, then a faint unlit green
    // overlay of the same geometry, so it still reads in a dark room. The
    // colour material is unskinned, so an animated model gets no overlay
    // rather than one in its bind pose.
    let glow_pass = !model.is_animated();
    let mut objects = Vec::new();
    for mut object in model.clone_scene_objects() {
        object.set_transform(placement);
        object.set_transparency(Some(HOLOGRAM_TRANSPARENCY));
        if !glow_pass {
            objects.push(object);
            continue;
        }
        let mut glow = object.clone();
        glow.material = Rc::new(std::cell::RefCell::new(
            engine::scene::color_material::create(vec3(0.25, 0.95, 0.55)),
        ));
        glow.set_transparency(Some(HOLOGRAM_GLOW_TRANSPARENCY));
        glow.set_depth_write(false);
        objects.push(object);
        objects.push(glow);
    }
    crate::util::tag_render_source(&mut objects, crate::util::render_source::MFD_HOLOGRAM);
    objects
}

/// A face-pixel rect as a transform of the centered unit quad, `lift` metres
/// above the front surface.
fn face_rect_transform(rect: Rect, lift: f32) -> Matrix4<f32> {
    let c = rect.center();
    Matrix4::from_translation(vec3(
        (c.x / FACE_PX.x - 0.5) * FACE_M.x,
        (0.5 - c.y / FACE_PX.y) * FACE_M.y,
        THICKNESS_M / 2.0 + lift,
    )) * Matrix4::from_nonuniform_scale(
        rect.w / FACE_PX.x * FACE_M.x,
        rect.h / FACE_PX.y * FACE_M.y,
        1.0,
    )
}

fn build_body() -> [Rc<engine::scene::mesh::Mesh>; 3] {
    let outline = rounded_rect(FACE_M / 2.0, CORNER_M, 6);
    let z = THICKNESS_M / 2.0;
    let v = |p: Vector2<f32>, z: f32| VertexPosition {
        position: vec3(p.x, p.y, z),
    };
    let (mut front, mut back, mut rim) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        // Convex outline: fans from the center close both faces.
        front.extend([v(vec2(0.0, 0.0), z), v(a, z), v(b, z)]);
        back.extend([v(vec2(0.0, 0.0), -z), v(b, -z), v(a, -z)]);
        rim.extend([v(a, -z), v(b, -z), v(b, z), v(a, -z), v(b, z), v(a, z)]);
    }
    [front, back, rim].map(|v| Rc::new(engine::scene::mesh::create(v)))
}

/// Counter-clockwise outline of a rectangle with `half` extents and rounded
/// corners of radius `r`, `segments` steps per corner.
fn rounded_rect(half: Vector2<f32>, r: f32, segments: usize) -> Vec<Vector2<f32>> {
    let corners = [
        (vec2(half.x - r, half.y - r), 0.0),
        (vec2(-half.x + r, half.y - r), 90.0),
        (vec2(-half.x + r, -half.y + r), 180.0),
        (vec2(half.x - r, -half.y + r), 270.0),
    ];
    corners
        .iter()
        .flat_map(|(center, start)| {
            (0..=segments).map(move |s| {
                let angle = (start + 90.0 * s as f32 / segments as f32).to_radians();
                center + vec2(angle.cos(), angle.sin()) * r
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Rotation;

    #[test]
    fn bar_readouts_sit_in_the_wells_and_buttons_in_the_recess() {
        let v = viewports(None);
        assert_eq!(v.len(), 2 + BUTTON_SOURCES.len());
        let inside = |a: Rect, b: Rect| {
            a.x >= b.x - 1e-3
                && a.y >= b.y - 1e-3
                && a.x + a.w <= b.x + b.w + 1e-3
                && a.y + a.h <= b.y + b.h + 1e-3
        };
        for (i, well) in ART_WELLS.into_iter().enumerate() {
            assert!(inside(v[i].dst, art_to_face(well)));
        }
        let recess = art_to_face(ART_RECESS);
        for pair in v[2..].windows(2) {
            assert!(inside(pair[0].dst, recess));
            assert!((pair[0].dst.x + pair[0].dst.w - pair[1].dst.x).abs() < 1e-3);
        }
    }

    #[test]
    fn a_face_hit_maps_into_the_mfd_slot_and_the_bezel_maps_nowhere() {
        let mfd = Rect::new(2.0, 124.0, 188.0, 296.0);
        let v = viewports(Some(mfd));
        let screen_center = SCREEN.center();
        let hit = crate::ui::canvas_viewport::target_to_canvas(&v, screen_center).unwrap();
        assert!(mfd.contains(hit));
        assert_eq!(
            crate::ui::canvas_viewport::target_to_canvas(&v, vec2(1.0, 1.0)),
            None
        );
    }

    #[test]
    fn face_panel_looks_out_of_either_palm() {
        let level = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        for (hand, side) in [(0, 1.0), (1, -1.0)] {
            let panel = face_panel(held_transform(Vector3::zero(), level, hand)).unwrap();
            let n = panel.normal();
            assert!((n.x - side).abs() < 1e-4, "hand {hand}: normal {n:?}");
            // The screen's top points along the fingers.
            let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
            assert!((up.z + 1.0).abs() < 1e-4);
        }
    }
}
