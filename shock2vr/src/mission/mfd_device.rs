//! Experimental handheld composition of the existing MFD. Tiles translate whole
//! native panels; glyph layout and widget geometry remain owned by the shared UI.
use crate::ui::canvas_viewport::{CanvasViewport, target_to_canvas};
use crate::ui::{HAlign, Rect, UiCanvas, VAlign, WorldPanel};
use cgmath::{InnerSpace, Quaternion, Rotation, Vector2, Vector3, vec2, vec3};

mod grip;

pub const SIZE: Vector2<f32> = Vector2::new(268.0, 376.0);
pub const SCREEN: Rect = Rect::new(8.0, 8.0, 252.0, 296.0);

pub fn panel(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    grip: Option<&crate::vr_grip::ResolvedGrip>,
    hand: usize,
) -> Option<WorldPanel> {
    let grip = grip?;
    let display = face_size() * grip.item_scale;
    let anchor = vec3(grip.anchor[0], grip.anchor[1], grip.anchor[2]);
    let scale = display.x / crate::tricorder::width();
    let center = scaled_grip_point(vec3(0.0, 0.0, 0.0), anchor, grip.item_scale, scale);
    let base = WorldPanel {
        center: position
            + rotation.rotate_vector(grip.offset + grip.rotation.rotate_vector(center)),
        rotation: (rotation * grip.rotation).normalize(),
        size: display,
    };
    let contact = position
        + rotation
            .rotate_vector(grip.offset + grip.rotation.rotate_vector(anchor * grip.item_scale));
    Some(grip::place(
        base,
        contact,
        rotation,
        &grip::tuning(hand),
        grip::margin(hand),
    ))
}

fn scaled_grip_point(
    point: Vector3<f32>,
    anchor: Vector3<f32>,
    authored: f32,
    scale: f32,
) -> Vector3<f32> {
    anchor * authored + (point - anchor) * scale
}

fn grip_margin() -> f32 {
    crate::dev_params::get(crate::dev_params::VR_MFD_GRIP_MARGIN) / crate::METERS_PER_WORLD_UNIT
}

fn face_size() -> Vector2<f32> {
    SIZE * (crate::dev_params::get(crate::dev_params::VR_MFD_WIDTH)
        / SIZE.x
        / crate::METERS_PER_WORLD_UNIT)
}

/// The device hand and any hand carrying an item keep their world controls.
/// Disable their entire UI ray, not just clicking, so hover and ownership agree.
pub fn filter_pointer_hands(
    input: &mut crate::input_context::InputContext,
    device_hand: Option<usize>,
    carrying: [bool; 2],
) {
    for (index, hand) in [&mut input.left_hand, &mut input.right_hand]
        .into_iter()
        .enumerate()
    {
        if device_hand == Some(index) || carrying[index] {
            hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
            hand.trigger_value = 0.0;
            hand.squeeze_value = 0.0;
        }
    }
}

/// Drawing, tracking recovery, or emptying a hand cannot turn an already-held
/// trigger/squeeze into a UI press. This masks only the UI copy of the input.
pub struct PointerGate {
    blocked: [[bool; 2]; 2],
}

impl Default for PointerGate {
    fn default() -> Self {
        Self {
            blocked: [[true; 2]; 2],
        }
    }
}

impl PointerGate {
    pub fn filter(
        &mut self,
        input: &mut crate::input_context::InputContext,
        device_hand: Option<usize>,
        carrying: [bool; 2],
    ) {
        use cgmath::InnerSpace;
        let tracked = std::array::from_fn::<_, 2, _>(|i| {
            input.pose_tracking.is_none_or(|p| p.head && p.hands[i])
        });
        for (i, hand) in [&mut input.left_hand, &mut input.right_hand]
            .into_iter()
            .enumerate()
        {
            let eligible = device_hand != Some(i)
                && !carrying[i]
                && tracked[i]
                && hand.rotation.magnitude2() > 0.0001;
            for (button, value) in [&mut hand.trigger_value, &mut hand.squeeze_value]
                .into_iter()
                .enumerate()
            {
                self.blocked[i][button] = !eligible
                    || (self.blocked[i][button] && *value > crate::ui::VR_TRIGGER_THRESHOLD);
                if self.blocked[i][button] {
                    *value = 0.0;
                }
            }
        }
        filter_pointer_hands(input, device_hand, carrying);
    }
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

/// The body keeps its physical pose and scale when the optional map expands.
/// Only the interaction/render canvas gains space above it.
#[derive(Clone, Copy)]
pub struct Layout {
    pub size: Vector2<f32>,
    pub body_offset: Vector2<f32>,
    pub screen: Option<CanvasViewport>,
}

impl Layout {
    fn new(screen: Option<Rect>, wide: bool) -> Self {
        let mut layout = Self {
            size: SIZE,
            body_offset: vec2(0.0, 0.0),
            screen: None,
        };
        if let Some(src) = screen {
            if src.w > SCREEN.w && wide {
                let width = 436.0;
                let height = width * src.h / src.w;
                layout.body_offset = vec2((width - SIZE.x) * 0.5, height + 8.0);
                layout.size = vec2(width, layout.body_offset.y + SIZE.y);
                layout.screen = Some(CanvasViewport::fit(src, Rect::new(0.0, 0.0, width, height)));
            } else if src.w > SCREEN.w {
                // Turn the entire map, including its close button and text.
                // The user turns the instrument sideways to read it.
                layout.screen = Some(CanvasViewport::fit_rotated(
                    src,
                    Rect::new(8.0, 8.0, 188.0, 296.0),
                    1,
                ));
            } else {
                layout.screen = Some(CanvasViewport::fit(
                    src,
                    Rect::new(SCREEN.x, SCREEN.y, src.w.min(SCREEN.w), src.h.min(SCREEN.h)),
                ));
            }
        }
        layout
    }

    pub fn surface_panel(self, body: WorldPanel) -> WorldPanel {
        let scale = body.size.x / SIZE.x;
        let shift = self.size * 0.5 - self.body_offset - SIZE * 0.5;
        WorldPanel {
            center: body.center
                + body
                    .rotation
                    .rotate_vector(vec3(shift.x, -shift.y, 0.0) * scale),
            rotation: body.rotation,
            size: self.size * scale,
        }
    }

    pub fn contains_surface(self, point: Vector2<f32>) -> bool {
        Rect::new(self.body_offset.x, self.body_offset.y, SIZE.x, SIZE.y).contains(point)
            || self.screen.is_some_and(|v| v.dst.contains(point))
    }
}

pub fn layout(screen: Option<Rect>) -> Layout {
    Layout::new(
        screen,
        crate::dev_params::get_bool(crate::dev_params::VR_MFD_MAP_WIDE),
    )
}

/// Source windows own both rendering and input: a ray maps continuously back
/// into the original host's controls rather than synthesizing fixed clicks.
pub fn viewports(screen: Option<Rect>) -> Vec<CanvasViewport> {
    let layout = layout(screen);
    let mut windows: Vec<_> = layout.screen.into_iter().collect();
    windows.extend(footer_tiles().map(|(src, dst)| CanvasViewport {
        turns: 0,
        src,
        dst: Rect::new(
            dst.x + layout.body_offset.x,
            dst.y + layout.body_offset.y,
            src.w,
            src.h,
        ),
    }));
    windows
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

pub fn to_native(point: Vector2<f32>, screen: Option<Rect>) -> Option<Vector2<f32>> {
    target_to_canvas(&viewports(screen), point)
}

pub fn point_from_native(point: Vector2<f32>, screen: Option<Rect>) -> Option<Vector2<f32>> {
    viewports(screen)
        .iter()
        .find(|v| v.src.contains(point))
        .map(|v| v.to_dst(Rect::new(point.x, point.y, 0.0, 0.0)).center())
}

pub fn from_native(rect: Rect, screen: Option<Rect>) -> Option<Rect> {
    viewports(screen)
        .iter()
        .find(|v| v.src.contains(rect.center()))
        .map(|v| v.to_dst(rect))
}

pub fn compose(native: UiCanvas, screen: Option<Rect>, target: Option<&str>) -> UiCanvas {
    let mut canvas = UiCanvas::new(SIZE);
    // Stepped corners give the prototype a solid, softly squared bezel without
    // an imported mesh. Sidecar art remains exposed beside the main body.
    canvas.fill(Rect::new(3.0, 0.0, 198.0, 374.0), [24, 31, 35]);
    canvas.fill(Rect::new(0.0, 3.0, 204.0, 368.0), [24, 31, 35]);
    canvas.fill(Rect::new(3.0, 304.0, 264.0, 70.0), [24, 31, 35]);
    canvas.fill(Rect::new(6.0, 6.0, 192.0, 362.0), [3, 8, 10]);
    if screen.is_none() {
        canvas.image(Rect::new(8.0, 8.0, 188.0, 296.0), "iface/query.pcx");
        // Match the retail query title and description wells (native panel
        // origin 2,124 translated to 8,8). Keep hints out of the image well.
        canvas.text_native_fit(
            Rect::new(32.0, 141.0, 129.0, 12.0),
            target.unwrap_or("SCANNER"),
            crate::ui::MFD_FONT,
            HAlign::Left,
            VAlign::Top,
        );
        let lines: &[&str] = if crate::dev_params::get_bool(crate::dev_params::VR_MFD_FOCUS_SCAN) {
            if target.is_some() {
                &["Hold steady to scan", "this object."]
            } else {
                &["Point at an object", "to scan it."]
            }
        } else if target.is_some() {
            &["Pull trigger to scan", "this object."]
        } else {
            &[
                "Point at an object,",
                "then pull the trigger",
                "to scan it.",
            ]
        };
        for (i, line) in lines.iter().enumerate() {
            canvas.text_native_fit(
                Rect::new(23.0, 161.0 + i as f32 * 12.0, 123.0, 12.0),
                line,
                crate::ui::MFD_FONT,
                HAlign::Left,
                VAlign::Top,
            );
        }
    }
    // Mirror only the housing bitmap: the raised end supports the right HRM
    // plug, while every icon, label and hit target remains upright.
    canvas.cropped_image(
        Rect::new(8.0, 306.0, 260.0, 64.0),
        "ammofull.pcx",
        Rect::new(260.0, 0.0, -260.0, 64.0),
        vec2(260.0, 64.0),
    );
    let layout = layout(screen);
    if layout.body_offset != vec2(0.0, 0.0) {
        let mut expanded = UiCanvas::new(layout.size);
        expanded.project(
            canvas,
            vec![CanvasViewport {
                src: Rect::new(0.0, 0.0, SIZE.x, SIZE.y),
                dst: Rect::new(layout.body_offset.x, layout.body_offset.y, SIZE.x, SIZE.y),
                turns: 0,
            }],
        );
        canvas = expanded;
    }
    canvas.project(native, viewports(screen));
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn landscape_modes_preserve_the_body_and_exclude_the_gap() {
        use cgmath::InnerSpace;
        let source = Some(Rect::new(2.0, 124.0, 636.0, 296.0));
        let body = WorldPanel {
            center: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            size: vec2(0.14, 0.14 * SIZE.y / SIZE.x),
        };
        for wide in [false, true] {
            let layout = Layout::new(source, wide);
            let surface = layout.surface_panel(body);
            for point in [vec2(8.0, 8.0), vec2(136.0, 341.0)] {
                let original = crate::ui::canvas_to_panel_world(SIZE, &body, point);
                let expanded = crate::ui::canvas_to_panel_world(
                    layout.size,
                    &surface,
                    point + layout.body_offset,
                );
                assert!((original - expanded).magnitude() < 0.00001);
            }
            let map = layout.screen.unwrap();
            assert_eq!(map.turns, if wide { 0 } else { 1 });
            assert!(layout.contains_surface(map.dst.center()));
            assert!(
                layout.contains_surface(layout.body_offset + vec2(2.0, 2.0)),
                "bezel still owns input"
            );
            if wide {
                assert!(map.dst.y + map.dst.h < layout.body_offset.y);
                assert!(!layout.contains_surface(vec2(1.0, layout.size.y - 1.0)));
            } else {
                assert_eq!(layout.size, SIZE);
            }
        }
    }

    #[test]
    fn resizing_the_device_keeps_the_authored_grip_contact_fixed() {
        let anchor = vec3(0.1, -0.02, 0.18);
        for scale in [0.3, 0.7, 1.2] {
            assert_eq!(scaled_grip_point(anchor, anchor, 1.0, scale), anchor);
            let edge = scaled_grip_point(anchor + vec3(0.2, 0.0, 0.0), anchor, 1.0, scale);
            assert!((edge.x - anchor.x - 0.2 * scale).abs() < 0.0001);
        }
    }

    #[test]
    fn screen_entry_and_recovery_wait_for_physical_release() {
        let mut gate = PointerGate::default();
        let mut raw = crate::input_context::InputContext::default();
        raw.right_hand.rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        raw.right_hand.trigger_value = 1.0;
        let sample =
            |gate: &mut PointerGate, raw: &crate::input_context::InputContext, carrying| {
                let mut input = raw.clone();
                gate.filter(&mut input, Some(0), [false, carrying]);
                input.right_hand.trigger_value
            };
        assert_eq!(sample(&mut gate, &raw, false), 0.0);
        assert_eq!(sample(&mut gate, &raw, false), 0.0);
        raw.right_hand.trigger_value = 0.0;
        assert_eq!(sample(&mut gate, &raw, false), 0.0);
        raw.right_hand.trigger_value = 1.0;
        assert_eq!(sample(&mut gate, &raw, false), 1.0);
        assert_eq!(sample(&mut gate, &raw, true), 0.0);
        assert_eq!(sample(&mut gate, &raw, false), 0.0);
        // Masking screen input never edits the original gun/world input.
        assert_eq!(raw.right_hand.trigger_value, 1.0);
    }

    #[test]
    fn only_the_empty_non_device_hand_has_a_screen_ray() {
        for device_hand in 0..2 {
            for carrying in [[false, false], [true, false], [false, true], [true, true]] {
                let mut input = crate::input_context::InputContext::default();
                let tracked = Quaternion::new(1.0, 0.0, 0.0, 0.0);
                for hand in [&mut input.left_hand, &mut input.right_hand] {
                    hand.rotation = tracked;
                    hand.trigger_value = 1.0;
                    hand.squeeze_value = 1.0;
                }
                filter_pointer_hands(&mut input, Some(device_hand), carrying);
                for (i, hand) in [&input.left_hand, &input.right_hand]
                    .into_iter()
                    .enumerate()
                {
                    let eligible = i != device_hand && !carrying[i];
                    assert_eq!(hand.rotation == tracked, eligible);
                    assert_eq!(hand.trigger_value > 0.0, eligible);
                    assert_eq!(hand.squeeze_value > 0.0, eligible);
                }
            }
        }
    }

    #[test]
    fn scanning_a_live_creature_cannot_open_its_loot() {
        let mut world = shipyard::World::new();
        let creature = world.add_entity((
            dark::properties::PropScripts {
                scripts: vec!["CreatureContainer".into()],
                inherits: false,
            },
            dark::properties::PropHitPoints { hit_points: 10 },
            dark::properties::Links::empty(),
        ));
        assert!(!opens_panel(&world, creature));
        world.add_component(creature, dark::properties::PropHitPoints { hit_points: 0 });
        assert!(opens_panel(&world, creature));
    }

    #[test]
    fn arbitrary_panels_and_footer_points_round_trip() {
        for src in [
            Rect::new(2.0, 124.0, 252.0, 296.0),
            Rect::new(450.0, 124.0, 188.0, 296.0),
            Rect::new(23.0, 19.0, 636.0, 296.0),
            Rect::new(400.0, 70.0, 100.0, 330.0),
        ] {
            let window = viewports(Some(src))[0];
            assert!(window.dst.w <= SCREEN.w && window.dst.h <= SCREEN.h);
            assert!(
                (window.dst.w / window.dst.h
                    - if window.turns % 2 == 0 {
                        src.w / src.h
                    } else {
                        src.h / src.w
                    })
                .abs()
                    < 0.001
            );
            for p in [src.center(), vec2(src.x + 1.0, src.y + 1.0)] {
                let mapped = point_from_native(p, Some(src)).unwrap();
                let round_trip = to_native(mapped, Some(src)).unwrap();
                assert!((round_trip.x - p.x).abs() < 0.001);
                assert!((round_trip.y - p.y).abs() < 0.001);
            }
        }
        for (src, _) in footer_tiles() {
            // An arbitrary point in each control, not a synthetic centre click.
            let p = vec2(src.x + 3.0, src.y + 5.0);
            let mapped = point_from_native(p, None).unwrap();
            assert_eq!(to_native(mapped, None), Some(p));
        }
        assert_eq!(to_native(vec2(2.0, 2.0), None), None);
    }
}

/// Scanning never picks up, fires, purchases or activates an arbitrary prop.
/// Only existing reader/panel scripts receive Frob; all other objects are queried.
pub fn opens_panel(world: &shipyard::World, entity: shipyard::EntityId) -> bool {
    super::personal_card::is_reader(world, entity)
        || (crate::scripts::script_util::entity_has_script(world, entity, "CreatureContainer")
            && crate::scripts::gui::creature_is_lootable(world, entity))
        || [
            "ContainerScript",
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
    focus: Option<(shipyard::EntityId, f32, bool)>,
}
impl Default for Scanner {
    fn default() -> Self {
        Self {
            pressed: true,
            focus: None,
        }
    }
}
impl Scanner {
    /// A stable target commits once; looking away or at another object rearms it.
    pub fn focus(
        &mut self,
        target: Option<shipyard::EntityId>,
        dt: f32,
    ) -> Option<shipyard::EntityId> {
        let Some(target) = target else {
            self.focus = None;
            return None;
        };
        if self.focus.is_none_or(|(previous, _, _)| previous != target) {
            self.focus = Some((target, 0.0, false));
        }
        let (_, elapsed, committed) = self.focus.as_mut().unwrap();
        *elapsed += dt.max(0.0);
        if !*committed && *elapsed >= 0.4 {
            *committed = true;
            Some(target)
        } else {
            None
        }
    }

    pub fn trigger(&mut self, enabled: bool, pressed: bool) -> bool {
        if !enabled {
            self.focus = None;
        }
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

/// Reuse the real model geometry, but give this copy a private blank material.
/// All candidates have a broad XZ face; preserve their aspect and thickness.
pub fn body(
    assets: &mut engine::assets::asset_cache::AssetCache,
    face: cgmath::Matrix4<f32>,
    hand: Option<usize>,
) -> Vec<engine::scene::SceneObject> {
    use cgmath::{Deg, EuclideanSpace, Matrix4};
    use engine::{
        scene::{SceneObjectDebugTag, basic_material},
        texture::TextureTrait,
    };
    use std::{cell::RefCell, rc::Rc};
    if crate::dev_params::get(crate::dev_params::VR_MFD_BODY).round() as u8 == 3 {
        return stepped_frame(
            face,
            hand.map(|i| grip::tuning(i).edge)
                .unwrap_or(grip::Edge::Bottom),
            hand.map(grip::margin).unwrap_or_else(grip_margin),
        );
    }
    let Some((model, scale, name)) = retail_body(assets) else {
        return vec![];
    };
    let bounds = model.bounding_box().unwrap();
    let size = bounds.max - bounds.min;
    let display = face_size();
    let center = (bounds.min.to_vec() + bounds.max.to_vec()) * 0.5;
    let transform =
        face * Matrix4::from_translation(vec3(
            0.0,
            0.0,
            -size.y * scale * 0.5 - 0.0015 / crate::METERS_PER_WORLD_UNIT,
        )) * Matrix4::from_angle_x(Deg(-90.0))
            * Matrix4::from_scale(scale)
            * Matrix4::from_translation(-center);
    thread_local! {
        static BLANK: Rc<engine::texture::Texture> = Rc::new(engine::texture::init_from_memory(
            engine::texture_format::RawTextureData {
                bytes: vec![26, 30, 33, 255], width: 1, height: 1,
                format: engine::texture_format::PixelFormat::RGBA,
            }));
    }
    let material = BLANK.with(|texture| {
        Rc::new(RefCell::new(basic_material::create_with_fixed_ambient(
            texture.clone() as Rc<dyn TextureTrait>,
            0.8,
            0.0,
        )))
    });
    let mut objects: Vec<_> = model
        .clone_scene_objects()
        .into_iter()
        .map(|mut object| {
            object.material = material.clone();
            object.set_transform(transform);
            object.set_debug_tag(Some(Rc::new(SceneObjectDebugTag {
                entity_id: None,
                name: None,
                model: Some(name.into()),
                source: Some("mfd_body".into()),
            })));
            object
        })
        .collect();
    let root = face
        * Matrix4::from_angle_z(Deg(180.0))
        * Matrix4::from_translation(vec3(0.0, grip_margin() * 0.5, 0.0));
    let extra = size.y * scale + 0.0015 / crate::METERS_PER_WORLD_UNIT
        - crate::tricorder::DEPTH_M / crate::METERS_PER_WORLD_UNIT;
    let mut lens = crate::tricorder::lens_objects(display.x);
    for object in &mut lens {
        object.set_transform(
            root * Matrix4::from_translation(vec3(0.0, 0.0, -extra)) * object.get_transform(),
        );
    }
    objects.extend(lens);
    objects
}

fn retail_body(
    assets: &mut engine::assets::asset_cache::AssetCache,
) -> Option<(std::rc::Rc<dark::model::Model>, f32, &'static str)> {
    let name = match crate::dev_params::get(crate::dev_params::VR_MFD_BODY).round() as u8 {
        1 => "upgrade.bin",
        2 => "magci.bin",
        _ => "scipass.bin",
    };
    let model =
        assets.get_opt::<_, dark::model::Model, _>(&dark::importers::MODELS_IMPORTER, name)?;
    let Some(bounds) = model.bounding_box() else {
        return None;
    };
    let size = bounds.max - bounds.min;
    let display = face_size();
    // Cover the complete canvas with a small rim. Never stretch the asset.
    let scale = (display.x / size.x.max(0.001))
        .max((display.y + grip_margin()) / size.z.max(0.001))
        * 1.045;
    Some((model, scale, name))
}

/// The rendered back lens and physics ray use this exact same local mount.
pub fn scanner_pose(
    assets: &mut engine::assets::asset_cache::AssetCache,
    panel: WorldPanel,
) -> (Vector3<f32>, Quaternion<f32>) {
    let mut lens = crate::tricorder::lens(face_size().x);
    if crate::dev_params::get(crate::dev_params::VR_MFD_BODY).round() as u8 != 3 {
        if let Some((model, scale, _)) = retail_body(assets) {
            let bounds = model.bounding_box().unwrap();
            let extra = (bounds.max.y - bounds.min.y) * scale
                + 0.0015 / crate::METERS_PER_WORLD_UNIT
                - crate::tricorder::DEPTH_M / crate::METERS_PER_WORLD_UNIT;
            lens.z -= extra;
        }
    }
    (
        panel.center
            + panel
                .rotation
                .rotate_vector(lens * (panel.size.x / face_size().x)),
        panel.rotation,
    )
}

/// Original stepped outline with a solid eight-millimetre backing.
fn stepped_frame(
    face: cgmath::Matrix4<f32>,
    edge: grip::Edge,
    margin: f32,
) -> Vec<engine::scene::SceneObject> {
    use cgmath::{Deg, Matrix4};
    use engine::scene::{SceneObject, SceneObjectDebugTag, color_material, cube};
    let display = face_size();
    let pixel = display.x / SIZE.x;
    let depth = 0.008 / crate::METERS_PER_WORLD_UNIT;
    let root = face
        * Matrix4::from_angle_z(Deg(180.0))
        * Matrix4::from_translation(vec3(0.0, grip_margin() * 0.5, 0.0));
    let extension = match edge {
        grip::Edge::Bottom => (0.0, -display.y * 0.5 - margin * 0.5, display.x, margin),
        grip::Edge::Top => (
            -32.0 * pixel,
            display.y * 0.5 + margin * 0.5,
            204.0 * pixel,
            margin,
        ),
        grip::Edge::Left => (-display.x * 0.5 - margin * 0.5, 0.0, margin, display.y),
        grip::Edge::Right => (
            102.0 * pixel + margin * 0.5,
            36.0 * pixel,
            64.0 * pixel + margin,
            304.0 * pixel,
        ),
    };
    let mut objects = crate::tricorder::frame_objects(display.x);
    objects.extend(crate::tricorder::lens_objects(display.x));
    if margin > 0.0 {
        let (x, y, width, height) = extension;
        let mut object = SceneObject::new(
            color_material::create(vec3(0.035, 0.05, 0.06)),
            Box::new(cube::create()),
        );
        object.set_transform(
            Matrix4::from_translation(vec3(x, y, -depth * 0.5))
                * Matrix4::from_nonuniform_scale(width, height, depth),
        );
        object.set_debug_tag(Some(std::rc::Rc::new(SceneObjectDebugTag {
            entity_id: None,
            name: None,
            model: Some("tricorder".into()),
            source: Some("mfd_body".into()),
        })));
        objects.push(object);
    }
    for object in &mut objects {
        object.set_transform(root * object.get_transform());
    }
    objects
}

pub fn body_frame(panel: WorldPanel) -> cgmath::Matrix4<f32> {
    // Retail comparison models retain their original upside-down XZ mapping.
    // The stepped frame cancels that legacy turn before placing its XY parts.
    cgmath::Matrix4::from_translation(panel.center)
        * cgmath::Matrix4::from(panel.rotation)
        * cgmath::Matrix4::from_scale(panel.size.x / face_size().x)
        * cgmath::Matrix4::from_translation(vec3(0.0, -grip_margin() * 0.5, 0.0))
        * cgmath::Matrix4::from_angle_z(cgmath::Deg(180.0))
}

#[cfg(test)]
mod scan_tests {
    use super::Scanner;
    #[test]
    fn focus_requires_dwell_and_rearms_only_after_target_changes() {
        let mut world = shipyard::World::new();
        let a = world.add_entity(());
        let b = world.add_entity(());
        let mut scan = Scanner::default();
        assert_eq!(scan.focus(Some(a), 0.1), None);
        assert_eq!(scan.focus(Some(a), 0.2), None);
        assert_eq!(scan.focus(Some(b), 0.1), None);
        assert_eq!(scan.focus(Some(b), 0.3), Some(b));
        assert_eq!(scan.focus(Some(b), 3.0), None);
        assert_eq!(scan.focus(None, 0.1), None);
        assert_eq!(scan.focus(Some(b), 0.2), None);
        assert_eq!(scan.focus(Some(b), 0.2), Some(b));
        scan.trigger(false, false);
        assert_eq!(scan.focus(Some(b), 0.1), None);
    }

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
    let size = face_size().x * (188.0 / SIZE.x) * 0.53;
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
