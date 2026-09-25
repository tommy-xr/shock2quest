//! Experimental handheld composition of the existing MFD. Tiles translate whole
//! native panels; glyph layout and widget geometry remain owned by the shared UI.
use crate::ui::{HAlign, Rect, UiCanvas, UiElement, VAlign, WorldPanel};
use cgmath::{Quaternion, Rotation, Vector2, Vector3, vec2, vec3};

pub const SIZE: Vector2<f32> = Vector2::new(268.0, 376.0);
pub const SCREEN: Rect = Rect::new(8.0, 8.0, 252.0, 296.0);

pub fn panel(position: Vector3<f32>, rotation: Quaternion<f32>) -> WorldPanel {
    // A 16 cm-wide main body; the HRM plug extends to its right. The screen
    // faces controller-local +Z, above the grip, like a handheld instrument.
    let scale = 0.16 / 188.0 / crate::METERS_PER_WORLD_UNIT;
    WorldPanel {
        center: position + rotation.rotate_vector(vec3(32.0 * scale, 153.0 * scale, 0.04)),
        rotation,
        size: SIZE * scale,
    }
}

pub fn source(character: bool) -> Rect {
    Rect::new(
        if character { 450.0 } else { 2.0 },
        124.0,
        if character { 188.0 } else { 252.0 },
        296.0,
    )
}

/// Retail controls retain their authored art and dimensions. The four-button
/// strip follows #1705 (RES, ?/MAP, LOG, MFD); ACCESS fills the spare recess
/// between the two balance wells and the strip.
fn footer_tiles() -> [(Rect, Vector2<f32>); 7] {
    [
        (Rect::new(185.0, 434.0, 36.0, 34.0), vec2(191.0, 326.0)),
        (Rect::new(224.0, 434.0, 36.0, 34.0), vec2(230.0, 326.0)),
        (Rect::new(422.0, 432.0, 38.0, 36.0), vec2(153.0, 323.0)),
        (Rect::new(117.0, 431.0, 32.0, 40.0), vec2(18.0, 321.0)),
        (Rect::new(150.0, 431.0, 32.0, 40.0), vec2(50.0, 321.0)),
        (Rect::new(383.0, 432.0, 38.0, 36.0), vec2(82.0, 323.0)),
        (Rect::new(460.0, 430.0, 32.0, 40.0), vec2(120.0, 321.0)),
    ]
}

pub fn buttons() -> [(Rect, Vector2<f32>, &'static str); 6] {
    [
        (
            Rect::new(82.0, 323.0, 38.0, 36.0),
            vec2(402.0, 450.0),
            "LOG",
        ),
        (
            Rect::new(153.0, 323.0, 38.0, 36.0),
            vec2(441.0, 450.0),
            "KEY",
        ),
        (
            Rect::new(120.0, 321.0, 32.0, 40.0),
            vec2(476.0, 450.0),
            "MFD",
        ),
        (
            Rect::new(18.0, 321.0, 32.0, 40.0),
            vec2(133.0, 450.0),
            "RES",
        ),
        (Rect::new(50.0, 321.0, 32.0, 18.0), vec2(166.0, 440.0), "?"),
        (
            Rect::new(50.0, 341.0, 32.0, 18.0),
            vec2(166.0, 460.0),
            "MAP",
        ),
    ]
}

/// Container scripts resolve their canvas before either renderer sees it.
/// A physical device is a screen, so it uses the retail loot canvas even in VR.
#[derive(shipyard::Unique)]
pub(crate) struct ScreenActive(pub bool);

pub(crate) fn screen_active(world: &shipyard::World) -> bool {
    world
        .borrow::<shipyard::UniqueView<ScreenActive>>()
        .is_ok_and(|v| v.0)
}

pub fn to_native(point: Vector2<f32>, character: bool) -> Option<Vector2<f32>> {
    for (rect, native, _) in buttons() {
        if rect.contains(point) {
            return Some(native);
        }
    }
    let src = source(character);
    let destination = Rect::new(SCREEN.x, SCREEN.y, src.w, src.h);
    destination
        .contains(point)
        .then(|| point + vec2(src.x - SCREEN.x, src.y - SCREEN.y))
}

pub fn point_from_native(point: Vector2<f32>, character: bool) -> Option<Vector2<f32>> {
    if let Some((rect, _, _)) = buttons()
        .into_iter()
        .find(|(_, native, _)| *native == point)
    {
        return Some(rect.center());
    }
    let src = source(character);
    src.contains(point)
        .then(|| point + vec2(SCREEN.x - src.x, SCREEN.y - src.y))
}

/// Debug geometry uses the same tile translation as composition/input.
pub fn from_native(rect: Rect, character: bool) -> Option<Rect> {
    for (destination, native, _) in buttons() {
        if rect.contains(native) {
            return Some(destination);
        }
    }
    let src = source(character);
    src.contains(rect.center()).then(|| {
        Rect::new(
            rect.x + SCREEN.x - src.x,
            rect.y + SCREEN.y - src.y,
            rect.w,
            rect.h,
        )
    })
}

pub fn compose(
    native: UiCanvas,
    character: bool,
    target: Option<&str>,
    occupied: bool,
) -> UiCanvas {
    let mut canvas = UiCanvas::new(SIZE);
    // Stepped corners give the prototype a solid, softly squared bezel without
    // an imported mesh. Sidecar art remains exposed beside the main body.
    canvas.fill(Rect::new(3.0, 0.0, 198.0, 374.0), [24, 31, 35]);
    canvas.fill(Rect::new(0.0, 3.0, 204.0, 368.0), [24, 31, 35]);
    canvas.fill(Rect::new(3.0, 304.0, 264.0, 70.0), [24, 31, 35]);
    canvas.fill(Rect::new(6.0, 6.0, 192.0, 362.0), [3, 8, 10]);
    if !occupied {
        canvas.image(Rect::new(8.0, 8.0, 188.0, 296.0), "iface/media.pcx");
        canvas.text_native_fit(
            Rect::new(18.0, 100.0, 140.0, 24.0),
            "SCAN MODE",
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas.text_native_fit(
            Rect::new(18.0, 132.0, 140.0, 24.0),
            target.unwrap_or("POINT AT AN OBJECT"),
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas.text_native_fit(
            Rect::new(18.0, 157.0, 140.0, 24.0),
            "PULL TRIGGER TO SCAN",
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
    }
    // Mirror only the housing bitmap: the raised end supports the right HRM
    // plug, while every icon, label and hit target remains upright.
    canvas.cropped_image(
        Rect::new(8.0, 306.0, 260.0, 64.0),
        "ammofull.pcx",
        Rect::new(260.0, 0.0, -260.0, 64.0),
        vec2(260.0, 64.0),
    );
    let src = source(character);
    for mut element in native.into_elements() {
        let rect = element.rect();
        let delta = if let Some((tile, destination)) =
            footer_tiles().into_iter().find(|(tile, _)| {
                rect.x >= tile.x
                    && rect.y >= tile.y
                    && rect.x + rect.w <= tile.x + tile.w
                    && rect.y + rect.h <= tile.y + tile.h
            }) {
            destination - vec2(tile.x, tile.y)
        } else if src.contains(rect.center()) {
            vec2(SCREEN.x - src.x, SCREEN.y - src.y)
        } else {
            continue;
        };
        match &mut element {
            UiElement::Image { position, .. }
            | UiElement::Bar { position, .. }
            | UiElement::Button { position, .. }
            | UiElement::Text { position, .. }
            | UiElement::Fill { position, .. } => *position += delta,
        }
        canvas.push(element);
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main_and_sidecar_keep_their_native_hit_coordinates() {
        assert_eq!(to_native(vec2(24.0, 218.0), false), Some(vec2(18.0, 334.0)));
        assert_eq!(
            to_native(vec2(221.0, 248.0), false),
            Some(vec2(215.0, 364.0))
        );
        assert_eq!(to_native(vec2(24.0, 28.0), true), Some(vec2(466.0, 144.0)));
        assert_eq!(to_native(vec2(240.0, 380.0), false), None);
    }
    #[test]
    fn utilities_and_balances_are_not_scanning_targets() {
        for (rect, native, _) in buttons() {
            assert_eq!(to_native(rect.center(), false), Some(native));
            assert_eq!(point_from_native(native, false), Some(rect.center()));
        }
        assert_eq!(to_native(vec2(205.0, 345.0), false), None);
    }
}

/// Scanning never picks up, fires, purchases or activates an arbitrary prop.
/// Only existing reader/panel scripts receive Frob; all other objects are queried.
pub fn opens_panel(world: &shipyard::World, entity: shipyard::EntityId) -> bool {
    super::personal_card::is_reader(world, entity)
        || [
            "ContainerScript",
            "CreatureContainer",
            "KeyPad",
            "KeyPadUnhackable",
            "HackableCrate",
            "ResearchableScript",
            "Chemical",
        ]
        .iter()
        .any(|script| crate::scripts::script_util::entity_has_script(world, entity, script))
}

/// One trigger pull commits one scan. Drawing/recovery starts disarmed.
pub struct Scanner {
    pressed: bool,
}
impl Default for Scanner {
    fn default() -> Self {
        Self { pressed: true }
    }
}
impl Scanner {
    pub fn trigger(&mut self, enabled: bool, pressed: bool) -> bool {
        let edge = enabled && pressed && !self.pressed;
        self.pressed = !enabled || pressed;
        edge
    }
}

pub fn beam(start: Vector3<f32>, end: Vector3<f32>) -> engine::scene::SceneObject {
    use engine::scene::{SceneObject, color_material, lines_mesh};
    let r = 0.008 / crate::METERS_PER_WORLD_UNIT;
    SceneObject::new(
        color_material::create(vec3(0.2, 1.0, 0.8)),
        Box::new(lines_mesh::create(
            vec![
                start,
                end,
                end - vec3(r, 0.0, 0.0),
                end + vec3(r, 0.0, 0.0),
                end - vec3(0.0, r, 0.0),
                end + vec3(0.0, r, 0.0),
            ]
            .into_iter()
            .map(|position| engine::scene::VertexPosition { position })
            .collect(),
        )),
    )
}

pub fn shell(panel: WorldPanel) -> engine::scene::SceneObject {
    use cgmath::Matrix4;
    use engine::scene::{SceneObject, color_material, cube};
    let mut object = SceneObject::new(
        color_material::create(vec3(0.035, 0.05, 0.06)),
        Box::new(cube::create()),
    );
    object.set_transform(
        panel.transform()
            * Matrix4::from_translation(vec3(-32.0 / SIZE.x, 0.0, -0.014))
            * Matrix4::from_nonuniform_scale(204.0 / SIZE.x, 1.0, 0.025),
    );
    object
}

pub fn footer_shell(panel: WorldPanel) -> engine::scene::SceneObject {
    use cgmath::Matrix4;
    use engine::scene::{SceneObject, color_material, cube};
    let mut object = SceneObject::new(
        color_material::create(vec3(0.035, 0.05, 0.06)),
        Box::new(cube::create()),
    );
    object.set_transform(
        panel.transform()
            * Matrix4::from_translation(vec3(0.0, -152.0 / SIZE.y, -0.014))
            * Matrix4::from_nonuniform_scale(1.0, 72.0 / SIZE.y, 0.025),
    );
    object
}

/// Resting instrument uses the card's authored belt frame and retrieval anchor.
pub fn belt_shell(transform: cgmath::Matrix4<f32>) -> engine::scene::SceneObject {
    use engine::scene::{SceneObject, color_material, cube};
    let mut object = SceneObject::new(
        color_material::create(vec3(0.035, 0.05, 0.06)),
        Box::new(cube::create()),
    );
    object.set_transform(
        transform
            * cgmath::Matrix4::from_nonuniform_scale(
                0.014 / crate::METERS_PER_WORLD_UNIT,
                0.28 / crate::METERS_PER_WORLD_UNIT,
                0.16 / crate::METERS_PER_WORLD_UNIT,
            ),
    );
    object
}

#[cfg(test)]
mod scan_tests {
    use super::Scanner;
    #[test]
    fn drawing_and_tracking_recovery_require_a_fresh_trigger_edge() {
        let mut scan = Scanner::default();
        assert!(!scan.trigger(true, true));
        assert!(!scan.trigger(true, false));
        assert!(scan.trigger(true, true));
        assert!(!scan.trigger(true, true));
        assert!(!scan.trigger(false, false));
        assert!(!scan.trigger(true, true));
        assert!(!scan.trigger(true, false));
        assert!(scan.trigger(true, true));
    }
}

/// A miniature of the real target mesh, never a new gameplay entity. The
/// bounding sphere fits at every rotation; authored pivots are recentered so
/// even large machines remain above the screen instead of orbiting the grip.
pub fn hologram(
    model: &dark::model::Model,
    panel: WorldPanel,
    seconds: f32,
) -> Vec<engine::scene::SceneObject> {
    use cgmath::{EuclideanSpace, InnerSpace, Matrix4};
    let Some(bounds) = model.bounding_box() else {
        return vec![];
    };
    let diameter = (bounds.max - bounds.min).magnitude();
    if !diameter.is_finite() || diameter <= 0.0001 {
        return vec![];
    }
    let size = 0.085 / crate::METERS_PER_WORLD_UNIT;
    let center = (bounds.min.to_vec() + bounds.max.to_vec()) * 0.5;
    // Compare a projection from the display glass with the original top-edge
    // placement. This changes only the physical miniature, never canvas layout.
    let (y, z) = if crate::dev_params::get_bool(crate::dev_params::VR_MFD_HOLOGRAM_SCREEN) {
        // Center over the upper image area, with the fitted sphere's nearest
        // point 8 mm above the glass even as it rotates.
        (
            (SIZE.y * 0.5 - (SCREEN.y + 70.0)) * panel.size.y / SIZE.y,
            size * 0.5 + 0.008 / crate::METERS_PER_WORLD_UNIT,
        )
    } else {
        (
            panel.size.y * 0.5 + size * 0.5 + 0.012 / crate::METERS_PER_WORLD_UNIT,
            0.025 / crate::METERS_PER_WORLD_UNIT,
        )
    };
    let anchor = vec3(-32.0 / SIZE.x * panel.size.x, y, z);
    let root = Matrix4::from_translation(panel.center)
        * Matrix4::from(panel.rotation)
        * Matrix4::from_translation(anchor)
        * Matrix4::from_angle_y(cgmath::Deg((seconds * 35.0) % 360.0))
        * Matrix4::from_scale(size / diameter)
        * Matrix4::from_translation(-center);
    // Keep the original texture/skin material and its recognizable detail.
    // Per-object overrides do not alter the world object's shared material.
    let lights = std::rc::Rc::new(
        engine::scene::light::LightArray::new().with_object_lighting(vec3(0.35, 1.0, 0.7), 1.0),
    );
    let mut objects = model.to_scene_objects().clone();
    for object in &mut objects {
        object.set_transform(root);
        object.set_transparency(Some(0.45));
        object.set_depth_write(false);
        object.set_depth_bias(false);
        object.set_lights(Some(lights.clone()));
        object.set_debug_tag(Some(std::rc::Rc::new(engine::scene::SceneObjectDebugTag {
            entity_id: None,
            name: None,
            model: None,
            source: Some("mfd_hologram".into()),
        })));
    }
    // Dark authored textures can disappear against a dark room. A faint unlit
    // copy keeps the projection readable while the textured copy supplies its
    // detail. Retain skinning for creature meshes (the sensory-psi rim shader).
    let glow: Vec<_> = objects
        .iter()
        .map(|object| {
            let material = object.material.borrow();
            let color = vec3(0.05, 1.0, 0.65);
            let replacement = if let Some(skinned) = material
                .as_any()
                .downcast_ref::<engine::scene::SkinnedMaterial>()
            {
                skinned.silhouette(color)
            } else {
                engine::scene::color_material::create(color)
            };
            let mut glow = object.clone();
            glow.material = std::rc::Rc::new(std::cell::RefCell::new(replacement));
            glow.set_transparency(Some(0.80));
            glow
        })
        .collect();
    objects.extend(glow);
    objects
}
