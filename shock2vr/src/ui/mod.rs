//! Placement-agnostic 2D UI canvas.
//!
//! UI is described in **canvas pixels** at a fixed virtual resolution (e.g.
//! 640x480, matching the original SS2 art). The content never encodes where it
//! is shown; a *presenter* maps the canvas either to a screen-space overlay
//! (`render_screen_space`) or a world-space panel (`render_world_space`). A
//! future render-target presenter can consume the same description for
//! diegetic screens.
//!
//! Alignment is resolved at render time (the renderer has the font, so it
//! measures text and centers it), which keeps layout out of hand-tuned
//! coordinates.

// Foundational UI toolkit: a few API items (the full alignment variants and
// geometry/introspection helpers) are intentionally complete ahead of their
// call sites.
#![allow(dead_code)]

use std::rc::Rc;

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rotation, Vector2, Vector3, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    ellipsize, measure_text_width,
    scene::SceneObject,
    texture::{Texture, TextureOptions, TextureTrait},
};
use shipyard::EntityId;

use crate::vr_config::Handedness;

/// A rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, p: Vector2<f32>) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }

    pub fn center(&self) -> Vector2<f32> {
        vec2(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// How a canvas maps onto a target when their aspect ratios differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    /// Fill the target, stretching each axis independently (may distort).
    Stretch,
    /// Uniform scale that fits the canvas inside the target, centered, leaving
    /// empty bars on the longer axis (letterbox / pillarbox).
    PreserveAspect,
}

/// Canvas->target mapping such that `target_px = canvas_px * scale + offset`.
fn fit(
    canvas: Vector2<f32>,
    target: Vector2<f32>,
    mode: ScaleMode,
) -> (Vector2<f32>, Vector2<f32>) {
    match mode {
        ScaleMode::Stretch => (
            vec2(target.x / canvas.x, target.y / canvas.y),
            vec2(0.0, 0.0),
        ),
        ScaleMode::PreserveAspect => {
            let s = (target.x / canvas.x).min(target.y / canvas.y);
            let offset = vec2(
                (target.x - canvas.x * s) / 2.0,
                (target.y - canvas.y * s) / 2.0,
            );
            (vec2(s, s), offset)
        }
    }
}

/// Map a normalized pointer (`InputContext::pointer`, `[0,1]`) to canvas pixels
/// for a `canvas_size` canvas shown on `screen_size` under `mode`. Returns
/// `None` when the pointer falls in the letterbox bars (outside the canvas).
pub fn pointer_to_canvas(
    canvas_size: Vector2<f32>,
    normalized: Vector2<f32>,
    screen_size: Vector2<f32>,
    mode: ScaleMode,
) -> Option<Vector2<f32>> {
    let (scale, offset) = fit(canvas_size, screen_size, mode);
    let screen = vec2(normalized.x * screen_size.x, normalized.y * screen_size.y);
    let canvas = vec2(
        (screen.x - offset.x) / scale.x,
        (screen.y - offset.y) / scale.y,
    );
    if canvas.x < 0.0 || canvas.y < 0.0 || canvas.x > canvas_size.x || canvas.y > canvas_size.y {
        None
    } else {
        Some(canvas)
    }
}

/// Map a canvas-pixel rect to normalized screen coordinates (`[0,1]` per
/// axis, origin top-left) for a `canvas_size` canvas shown on `screen_size`
/// under `mode` - the rect analogue (and inverse) of [`pointer_to_canvas`].
/// Lets clients (e.g. the debug runtime's `GET /v1/ui`) aim a normalized
/// pointer at a canvas rect without re-deriving the letterbox math.
/// A flat UI panel placed in the world: where it is, which way it faces, and
/// how big it is in metres. Frontend scenes in VR present their canvas on one
/// of these instead of on the screen.
#[derive(Debug, Clone, Copy)]
pub struct WorldPanel {
    /// Center of the panel, in world space.
    pub center: Vector3<f32>,
    /// Panel orientation. The panel's face normal is this rotation applied to
    /// +Z, so an identity rotation faces the default camera direction.
    pub rotation: Quaternion<f32>,
    /// Panel size in metres (width, height).
    pub size: Vector2<f32>,
}

impl WorldPanel {
    /// The panel's outward face normal.
    pub fn normal(&self) -> Vector3<f32> {
        self.rotation.rotate_vector(vec3(0.0, 0.0, 1.0))
    }

    /// Root transform for [`UiCanvas::render_world_space`].
    ///
    /// No facing correction is needed: the element path's own 180-degree
    /// rotation is about **Z** (in-plane, flipping the canvas's axes), so the
    /// quad's +Z normal is untouched and a panel whose
    /// [`normal`](Self::normal) faces the viewer renders toward them.
    pub fn transform(&self) -> Matrix4<f32> {
        Matrix4::from_translation(self.center)
            * Matrix4::from(self.rotation)
            * Matrix4::from_nonuniform_scale(self.size.x, self.size.y, 1.0)
    }
}

/// Intersect a pointing ray with `panel` and return where it lands, in canvas
/// pixels - the VR counterpart of [`pointer_to_canvas`].
///
/// `canvas_size` is the panel's authored pixel canvas (e.g. 640x480).
/// Returns `None` when the ray is parallel to the panel, points away from it,
/// or lands outside its bounds, so a caller can treat "not pointing at the
/// menu" the same way flat treats "cursor off the canvas".
pub fn ray_to_canvas(
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    ray_origin: Vector3<f32>,
    ray_direction: Vector3<f32>,
) -> Option<Vector2<f32>> {
    let normal = panel.normal();
    let denominator = ray_direction.dot(normal);
    // Parallel to the panel (or close enough that the intersection is
    // numerically meaningless).
    if denominator.abs() < 1e-6 {
        return None;
    }

    let distance = (panel.center - ray_origin).dot(normal) / denominator;
    // The panel is behind the ray, not in front of it.
    if distance <= 0.0 {
        return None;
    }

    let hit = ray_origin + ray_direction * distance;
    let local = hit - panel.center;
    // Undo the panel's rotation to get panel-local axes.
    let inverse = panel.rotation.conjugate();
    let local = inverse.rotate_vector(local);

    // Panel-local -> [0,1] with the origin at the top-left, matching the
    // canvas convention (y grows downward on the canvas, upward in the world).
    let u = local.x / panel.size.x + 0.5;
    let v = 0.5 - local.y / panel.size.y;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return None;
    }

    Some(vec2(u * canvas_size.x, v * canvas_size.y))
}

pub fn canvas_rect_to_screen(
    rect: Rect,
    canvas_size: Vector2<f32>,
    screen_size: Vector2<f32>,
    mode: ScaleMode,
) -> Rect {
    let (scale, offset) = fit(canvas_size, screen_size, mode);
    Rect::new(
        (rect.x * scale.x + offset.x) / screen_size.x,
        (rect.y * scale.y + offset.y) / screen_size.y,
        rect.w * scale.x / screen_size.x,
        rect.h * scale.y / screen_size.y,
    )
}

/// How a button changes while the pointer is inside its rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonHoverBehavior {
    None,
    Texture(String),
}

/// How an image's art is keyed and sized.
///
/// Dark's inventory object icons are authored at their own pixel size - a 1x1
/// item ships a 32x32 icon, a 1x3 weapon a ~34x99 one, and some are narrower
/// than their cell (the wrench is 22 wide) - and are blitted 1:1 into the
/// slot, not stretched to it. They also key transparency on palette index 0,
/// independent of that entry's RGB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageKind {
    /// Ordinary UI art: opaque, stretched to the element's rect.
    #[default]
    Ui,
    /// Object-icon art: palette index 0 is transparent, and the icon draws at
    /// its authored pixel size centered inside the element rect.
    /// The rect still defines the element's slot for layout and hit-testing.
    ObjectIcon,
    /// Object-icon art constrained to the element's rect. Palette index 0 is
    /// transparent as above, but oversized art is scaled down uniformly and
    /// centered so compact lists can show icons of every inventory footprint
    /// without stretching or overlapping adjacent rows. Smaller art remains
    /// at its authored size.
    ObjectIconFit,
}

impl ImageKind {
    pub(crate) fn transparent_index_0(self) -> bool {
        matches!(self, Self::ObjectIcon | Self::ObjectIconFit)
    }
}

/// One item in the shared 2D UI description language.
///
/// `TEvent` is presentation-agnostic: interactive panels attach their
/// Elm-style message type, while HUD/loading canvases use the default `()` and
/// simply never add a button. Positions and sizes are always canvas pixels
/// with a top-left origin.
#[derive(Clone, Debug)]
pub enum UiElement<TEvent = ()>
where
    TEvent: Clone,
{
    Image {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        alpha: f32,
        /// How the art is keyed and sized (see [`ImageKind`]).
        kind: ImageKind,
    },
    /// Horizontally-filling bar; `fill` (0..1) clips the texture from the left.
    Bar {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        fill: f32,
        alpha: f32,
    },
    Button {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        on_click: Option<TEvent>,
        on_grab: Option<(TEvent, TEvent)>,
        hover: ButtonHoverBehavior,
        alpha: f32,
        /// How the art is keyed and sized (see [`ImageKind`]).
        kind: ImageKind,
        /// The world entity this button represents, for UI introspection.
        entity: Option<EntityId>,
        /// Optional semantic label, for UI introspection and automation.
        label: Option<String>,
    },
    Text {
        position: Vector2<f32>,
        size: Vector2<f32>,
        text: String,
        font: String,
        /// Glyph-cell height in canvas pixels; `<= 0` means native font size.
        font_size: f32,
        h: HAlign,
        v: VAlign,
        alpha: f32,
        /// When set, text wider than its rect is shortened with a trailing
        /// ellipsis instead of spilling over neighbouring widgets. Only the
        /// screen-space path honours this (see [`UiCanvas::text_native_fit`]).
        fit_to_rect: bool,
    },
}

impl<TEvent> UiElement<TEvent>
where
    TEvent: Clone,
{
    pub fn rect(&self) -> Rect {
        let (position, size) = match self {
            Self::Image { position, size, .. }
            | Self::Bar { position, size, .. }
            | Self::Button { position, size, .. }
            | Self::Text { position, size, .. } => (*position, *size),
        };
        Rect::new(position.x, position.y, size.x, size.y)
    }

    pub fn click_event(&self) -> Option<&TEvent> {
        match self {
            Self::Button { on_click, .. } => on_click.as_ref(),
            _ => None,
        }
    }

    pub fn grab_event(&self, hand: Handedness) -> Option<&TEvent> {
        match self {
            Self::Button {
                on_grab: Some((left, right)),
                ..
            } => Some(if hand == Handedness::Left {
                left
            } else {
                right
            }),
            _ => None,
        }
    }
}

/// A resolution-independent 2D UI description in canvas pixels.
#[derive(Clone, Debug)]
pub struct UiCanvas<TEvent = ()>
where
    TEvent: Clone,
{
    size: Vector2<f32>,
    elements: Vec<UiElement<TEvent>>,
}

impl<TEvent> UiCanvas<TEvent>
where
    TEvent: Clone,
{
    pub fn with_events(size: Vector2<f32>) -> Self {
        Self {
            size,
            elements: Vec::new(),
        }
    }

    pub fn from_elements(size: Vector2<f32>, elements: Vec<UiElement<TEvent>>) -> Self {
        Self { size, elements }
    }

    pub fn size(&self) -> Vector2<f32> {
        self.size
    }

    pub fn elements(&self) -> &[UiElement<TEvent>] {
        &self.elements
    }

    pub fn into_elements(self) -> Vec<UiElement<TEvent>> {
        self.elements
    }

    pub fn push(&mut self, element: UiElement<TEvent>) -> &mut Self {
        self.elements.push(element);
        self
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Set the opacity (0..1) of the most recently added element. Chains after a
    /// builder call, e.g. `canvas.text(...).opacity(0.6)`.
    pub fn opacity(&mut self, opacity: f32) -> &mut Self {
        if let Some(last) = self.elements.last_mut() {
            let o = opacity.clamp(0.0, 1.0);
            match last {
                UiElement::Image { alpha, .. }
                | UiElement::Bar { alpha, .. }
                | UiElement::Button { alpha, .. }
                | UiElement::Text { alpha, .. } => *alpha = o,
            }
        }
        self
    }

    pub fn image(&mut self, rect: Rect, texture: &str) -> &mut Self {
        self.elements.push(UiElement::Image {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            alpha: 1.0,
            kind: ImageKind::Ui,
        });
        self
    }

    /// Add paletted object-icon art using Dark's palette-index-0 transparency.
    /// Ordinary UI images remain opaque through [`Self::image`].
    pub fn object_icon(&mut self, rect: Rect, texture: &str) -> &mut Self {
        self.elements.push(UiElement::Image {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            alpha: 1.0,
            kind: ImageKind::ObjectIcon,
        });
        self
    }

    /// Add object-icon art centered and aspect-fitted inside `rect`.
    pub fn fitted_object_icon(&mut self, rect: Rect, texture: &str) -> &mut Self {
        self.elements.push(UiElement::Image {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            alpha: 1.0,
            kind: ImageKind::ObjectIconFit,
        });
        self
    }

    pub fn bar(&mut self, rect: Rect, texture: &str, fill: f32) -> &mut Self {
        self.elements.push(UiElement::Bar {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            fill: fill.clamp(0.0, 1.0),
            alpha: 1.0,
        });
        self
    }

    /// Add a textured button. Its rectangle participates in
    /// [`click_at`](Self::click_at), and the last-painted matching button wins.
    pub fn button(&mut self, rect: Rect, texture: &str, on_click: TEvent) -> &mut Self {
        self.elements.push(UiElement::Button {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            on_click: Some(on_click),
            on_grab: None,
            hover: ButtonHoverBehavior::None,
            alpha: 1.0,
            entity: None,
            label: None,
            kind: ImageKind::Ui,
        });
        self
    }

    /// Resolve a click in canvas pixels. Elements are tested back-to-front so
    /// interaction follows paint order when button rectangles overlap.
    pub fn click_at(&self, point: Vector2<f32>) -> Option<TEvent> {
        self.elements
            .iter()
            .rev()
            .find(|element| element.click_event().is_some() && element.rect().contains(point))
            .and_then(UiElement::click_event)
            .cloned()
    }

    /// Add a text element sized in **canvas pixels** (`size` = the full
    /// glyph-cell height at the 640x480 canvas scale - `.FON` cells include the
    /// font's internal leading, so the visible cap ink is shorter than `size`).
    /// Pass `size <= 0.0` to render at the font's *native* pixel height
    /// (`base_height`); prefer [`text_native`](Self::text_native) for that.
    pub fn text(
        &mut self,
        rect: Rect,
        text: &str,
        font: &str,
        size: f32,
        h: HAlign,
        v: VAlign,
    ) -> &mut Self {
        self.push_text(rect, text, font, size, h, v, false)
    }

    /// Add text rendered at the font's **native pixel height** on the 640x480
    /// canvas - the way the Dark engine draws its bitmap `.FON` fonts (1:1, no
    /// scaling). This is the fidelity-correct default for UI text; callers only
    /// pick an explicit [`text`](Self::text) size when the original art
    /// deliberately scales a font. Height comes from the loaded font's
    /// `base_height` at render time (the `size <= 0.0` sentinel).
    pub fn text_native(
        &mut self,
        rect: Rect,
        text: &str,
        font: &str,
        h: HAlign,
        v: VAlign,
    ) -> &mut Self {
        self.text(rect, text, font, 0.0, h, v)
    }

    /// [`text_native`](Self::text_native) for text of unbounded length - a save
    /// name, a player-authored label - that must stay inside its rect. Anything
    /// too wide is shortened with a trailing ellipsis at render time, where the
    /// font (and so the real glyph widths) is available.
    pub fn text_native_fit(
        &mut self,
        rect: Rect,
        text: &str,
        font: &str,
        h: HAlign,
        v: VAlign,
    ) -> &mut Self {
        self.push_text(rect, text, font, 0.0, h, v, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_text(
        &mut self,
        rect: Rect,
        text: &str,
        font: &str,
        size: f32,
        h: HAlign,
        v: VAlign,
        fit_to_rect: bool,
    ) -> &mut Self {
        self.elements.push(UiElement::Text {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            text: text.to_owned(),
            font: font.to_owned(),
            font_size: size,
            h,
            v,
            alpha: 1.0,
            fit_to_rect,
        });
        self
    }

    /// Render the canvas as a screen-space overlay on `screen_size`, mapped via
    /// `mode` (stretch-to-fill or aspect-preserving letterbox).
    pub fn render_screen_space(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        mode: ScaleMode,
    ) -> Vec<SceneObject> {
        self.render_screen_space_with_pointer(asset_cache, screen_size, mode, None)
    }

    /// Render in screen space while resolving button hover art from a pointer
    /// expressed in canvas pixels.
    pub fn render_screen_space_with_pointer(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        mode: ScaleMode,
        pointer: Option<Vector2<f32>>,
    ) -> Vec<SceneObject> {
        let (scale, offset) = fit(self.size, screen_size, mode);
        let texture_options = TextureOptions {
            wrap: false,
            ..Default::default()
        };
        let mut objs = Vec::with_capacity(self.elements.len());

        for element in &self.elements {
            match element {
                UiElement::Image {
                    position,
                    size,
                    texture,
                    alpha,
                    kind,
                } => {
                    let tex = asset_cache.get_ext(
                        &TEXTURE_IMPORTER,
                        texture,
                        &TextureOptions {
                            wrap: false,
                            transparent_index_0: kind.transparent_index_0(),
                        },
                    );
                    let (drawn_at, drawn) = drawn_rect(*position, *size, texture_px(&tex), *kind);
                    objs.push(SceneObject::screen_space_quad2(
                        tex.clone() as Rc<dyn TextureTrait>,
                        vec2(
                            drawn_at.x * scale.x + offset.x,
                            drawn_at.y * scale.y + offset.y,
                        ),
                        vec2(drawn.x * scale.x, drawn.y * scale.y),
                        *alpha,
                    ));
                }
                UiElement::Button {
                    position,
                    size,
                    texture,
                    hover,
                    alpha,
                    kind,
                    ..
                } => {
                    let hovered = pointer.is_some_and(|point| {
                        Rect::new(position.x, position.y, size.x, size.y).contains(point)
                    });
                    let texture = match (hovered, hover) {
                        (true, ButtonHoverBehavior::Texture(hover_texture)) => hover_texture,
                        _ => texture,
                    };
                    let kind = *kind;
                    let tex = asset_cache.get_ext(
                        &TEXTURE_IMPORTER,
                        texture,
                        &TextureOptions {
                            wrap: false,
                            transparent_index_0: kind.transparent_index_0(),
                        },
                    );
                    let (drawn_at, drawn) = drawn_rect(*position, *size, texture_px(&tex), kind);
                    objs.push(SceneObject::screen_space_quad2(
                        tex.clone() as Rc<dyn TextureTrait>,
                        vec2(
                            drawn_at.x * scale.x + offset.x,
                            drawn_at.y * scale.y + offset.y,
                        ),
                        vec2(drawn.x * scale.x, drawn.y * scale.y),
                        *alpha,
                    ));
                }
                UiElement::Bar {
                    position,
                    size,
                    texture,
                    fill,
                    alpha: _,
                } => {
                    let tex = asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options);
                    let material = engine::scene::clipped_screen_material::create_screen_space(
                        tex.clone() as Rc<dyn TextureTrait>,
                        *fill,
                    );
                    let mut obj =
                        SceneObject::new(material, Box::new(engine::scene::quad::create()));
                    obj.set_local_transform(screen_space_quad_transform(
                        vec2(
                            position.x * scale.x + offset.x,
                            position.y * scale.y + offset.y,
                        ),
                        vec2(size.x * scale.x, size.y * scale.y),
                    ));
                    objs.push(obj);
                }
                UiElement::Text {
                    position,
                    size,
                    text,
                    font,
                    font_size,
                    h,
                    v,
                    alpha,
                    fit_to_rect,
                } => {
                    let font_obj = asset_cache.get(&FONT_IMPORTER, font).clone();
                    // `size <= 0` renders at the font's native pixel height, so
                    // Dark `.FON` bitmap fonts draw at their authored size (the
                    // way the original engine does) instead of an ad-hoc scale.
                    let canvas_size = if *font_size > 0.0 {
                        *font_size
                    } else {
                        font_obj.base_height()
                    };
                    let font_size = canvas_size * scale.y;

                    let rx = position.x * scale.x + offset.x;
                    let ry = position.y * scale.y + offset.y;
                    let rw = size.x * scale.x;
                    let rh = size.y * scale.y;

                    let fitted;
                    let text: &str = if *fit_to_rect {
                        fitted = ellipsize(&**font_obj, text, font_size, rw);
                        &fitted
                    } else {
                        text
                    };
                    let width = measure_text_width(&**font_obj, text, font_size);
                    let x = match h {
                        HAlign::Left => rx,
                        HAlign::Center => rx + (rw - width) / 2.0,
                        HAlign::Right => rx + rw - width,
                    };
                    let y = match v {
                        VAlign::Top => ry,
                        VAlign::Middle => ry + (rh - font_size) / 2.0,
                        VAlign::Bottom => ry + rh - font_size,
                    };

                    objs.push(SceneObject::screen_space_text(
                        text, font_obj, font_size, *alpha, x, y,
                    ));
                }
            }
        }

        objs
    }

    /// Present this canvas as a world-space panel. `root_transform` places a
    /// unit canvas in the world; pixel coordinates are normalized here. The
    /// authored elements are the same ones consumed by the screen presenter.
    pub fn render_world_space(
        &self,
        asset_cache: &mut AssetCache,
        root_transform: Matrix4<f32>,
        pointer: Option<Vector2<f32>>,
        force_alpha: Option<f32>,
        component_z_step: f32,
    ) -> Vec<SceneObject> {
        let texture_options = TextureOptions {
            wrap: false,
            ..Default::default()
        };
        let mut objects = Vec::with_capacity(self.elements.len());

        for (index, element) in self.elements.iter().enumerate() {
            let mut object = match element {
                UiElement::Image {
                    position,
                    size,
                    texture,
                    alpha,
                    kind,
                } => world_image(
                    asset_cache,
                    texture,
                    *position,
                    *size,
                    self.size,
                    force_alpha.unwrap_or(*alpha),
                    *kind,
                ),
                UiElement::Button {
                    position,
                    size,
                    texture,
                    hover,
                    alpha,
                    kind,
                    ..
                } => {
                    let hovered = pointer.is_some_and(|point| {
                        Rect::new(position.x, position.y, size.x, size.y).contains(point)
                    });
                    let texture = match (hovered, hover) {
                        (true, ButtonHoverBehavior::Texture(hover_texture)) => hover_texture,
                        _ => texture,
                    };
                    world_image(
                        asset_cache,
                        texture,
                        *position,
                        *size,
                        self.size,
                        force_alpha.unwrap_or(*alpha),
                        *kind,
                    )
                }
                UiElement::Bar {
                    position,
                    size,
                    texture,
                    fill,
                    ..
                } => {
                    let texture = asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options);
                    let material = engine::scene::clipped_screen_material::create(
                        texture.clone() as Rc<dyn TextureTrait>,
                        *fill,
                    );
                    let mut object =
                        SceneObject::new(material, Box::new(engine::scene::quad::create()));
                    object.set_local_transform(world_element_transform(
                        *position, *size, self.size, 0.0,
                    ));
                    object
                }
                UiElement::Text {
                    position,
                    text,
                    font,
                    alpha,
                    ..
                } => {
                    let font = asset_cache.get(&FONT_IMPORTER, font).clone();
                    let alpha = force_alpha.unwrap_or(*alpha);
                    let mut object =
                        SceneObject::world_space_text(text, font, (1.0 - alpha).clamp(0.0, 1.0));
                    object.set_local_transform(
                        Matrix4::from_angle_y(Deg(180.0))
                            * Matrix4::from_translation(vec3(
                                position.x / self.size.x - 0.5,
                                -position.y / self.size.y - 0.5,
                                0.01,
                            )),
                    );
                    object
                }
            };
            object.set_transform(
                root_transform
                    * Matrix4::from_translation(vec3(0.0, 0.0, -component_z_step * index as f32)),
            );
            objects.push(object);
        }

        objects
    }
}

impl UiCanvas<()> {
    pub fn new(size: Vector2<f32>) -> Self {
        Self::with_events(size)
    }
}

/// Replicate `SceneObject::screen_space_quad`'s transform so a manually-built
/// screen-space object (e.g. the clipped bar material) maps to the same pixels.
fn screen_space_quad_transform(position: Vector2<f32>, size: Vector2<f32>) -> Matrix4<f32> {
    Matrix4::from_translation(vec3(position.x, position.y, 0.0))
        * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
        * Matrix4::from_translation(vec3(0.5, 0.5, 0.0))
}

fn world_image(
    asset_cache: &mut AssetCache,
    texture: &str,
    position: Vector2<f32>,
    size: Vector2<f32>,
    canvas_size: Vector2<f32>,
    alpha: f32,
    kind: ImageKind,
) -> SceneObject {
    let texture = asset_cache
        .get_ext(
            &TEXTURE_IMPORTER,
            texture,
            &TextureOptions {
                wrap: false,
                transparent_index_0: kind.transparent_index_0(),
            },
        )
        .clone();
    let (position, size) = drawn_rect(position, size, texture_px(&texture), kind);
    let material =
        engine::scene::basic_material::create(texture as Rc<dyn TextureTrait>, 1.0, 1.0 - alpha);
    let mut object = SceneObject::new(material, Box::new(engine::scene::quad::create()));
    object.set_local_transform(world_element_transform(position, size, canvas_size, 0.0));
    object
}

/// The size an element's art actually draws at: its slot rect for ordinary UI
/// art, the icon's own authored pixels for [`ImageKind::ObjectIcon`], or those
/// authored pixels uniformly downscaled to fit for [`ImageKind::ObjectIconFit`].
pub(crate) fn drawn_rect(
    position: Vector2<f32>,
    size: Vector2<f32>,
    texture_px: Vector2<f32>,
    kind: ImageKind,
) -> (Vector2<f32>, Vector2<f32>) {
    match kind {
        ImageKind::Ui => (position, size),
        ImageKind::ObjectIcon => (position + centered_offset(size, texture_px), texture_px),
        ImageKind::ObjectIconFit => {
            let scale = (size.x / texture_px.x).min(size.y / texture_px.y).min(1.0);
            let fitted = texture_px * scale;
            (position + centered_offset(size, fitted), fitted)
        }
    }
}

/// Where to place art of `drawn` size inside a `slot`, so it sits centered
/// rather than flush against the slot's top-left corner.
///
/// Whole pixels only - a half-pixel origin would resample the icon's pixel art
/// - and never negative: art bigger than its slot stays anchored at the
/// top-left and overhangs to the right/bottom, which keeps it inside the panel.
fn centered_offset(slot: Vector2<f32>, drawn: Vector2<f32>) -> Vector2<f32> {
    vec2(
        (((slot.x - drawn.x) / 2.0).floor()).max(0.0),
        (((slot.y - drawn.y) / 2.0).floor()).max(0.0),
    )
}

/// A loaded texture's authored pixel dimensions.
fn texture_px(texture: &Texture) -> Vector2<f32> {
    vec2(texture.width() as f32, texture.height() as f32)
}

fn world_element_transform(
    position: Vector2<f32>,
    size: Vector2<f32>,
    canvas_size: Vector2<f32>,
    z: f32,
) -> Matrix4<f32> {
    let position = vec2(position.x / canvas_size.x, position.y / canvas_size.y);
    let size = vec2(size.x / canvas_size.x, size.y / canvas_size.y);
    Matrix4::from_angle_z(Deg(180.0))
        * Matrix4::from_translation(vec3(
            position.x - 0.5 + size.x / 2.0,
            position.y - 0.5 + size.y / 2.0,
            z,
        ))
        * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);
        assert!(r.contains(vec2(10.0, 20.0)));
        assert!(r.contains(vec2(60.0, 40.0)));
        assert!(r.contains(vec2(110.0, 60.0)));
        assert!(!r.contains(vec2(9.0, 40.0)));
        assert!(!r.contains(vec2(60.0, 61.0)));
    }

    #[test]
    fn canvas_buttons_hit_test_in_reverse_paint_order() {
        let mut canvas = UiCanvas::<&'static str>::with_events(vec2(100.0, 100.0));
        canvas.button(Rect::new(10.0, 10.0, 40.0, 40.0), "bottom.pcx", "bottom");
        canvas.button(Rect::new(20.0, 20.0, 40.0, 40.0), "top.pcx", "top");

        assert_eq!(canvas.click_at(vec2(25.0, 25.0)), Some("top"));
        assert_eq!(canvas.click_at(vec2(15.0, 15.0)), Some("bottom"));
        assert_eq!(canvas.click_at(vec2(80.0, 80.0)), None);
    }

    #[test]
    fn stretch_maps_pointer_independent_of_screen_size() {
        // Stretch: normalized maps straight to the canvas regardless of screen.
        let canvas = vec2(640.0, 480.0);
        let p = pointer_to_canvas(
            canvas,
            vec2(0.5, 0.5),
            vec2(1920.0, 1080.0),
            ScaleMode::Stretch,
        );
        assert_eq!(p, Some(vec2(320.0, 240.0)));
    }

    #[test]
    fn preserve_aspect_letterboxes_and_centers() {
        // 640x480 (4:3) canvas in a 1280x480 (wider) target: uniform scale 1.0,
        // 320px pillarbox bars on each side. Screen center maps to canvas center.
        let canvas = vec2(640.0, 480.0);
        let screen = vec2(1280.0, 480.0);
        let center = pointer_to_canvas(canvas, vec2(0.5, 0.5), screen, ScaleMode::PreserveAspect);
        assert_eq!(center, Some(vec2(320.0, 240.0)));

        // A point inside the left pillarbox bar is outside the canvas.
        let in_bar = pointer_to_canvas(canvas, vec2(0.1, 0.5), screen, ScaleMode::PreserveAspect);
        assert_eq!(in_bar, None);
    }

    #[test]
    fn canvas_rect_to_screen_letterboxes_and_roundtrips() {
        // 640x480 canvas pillarboxed on a 1280x480 screen: scale 1, 320px bars.
        let canvas = vec2(640.0, 480.0);
        let screen = vec2(1280.0, 480.0);
        let r = canvas_rect_to_screen(
            Rect::new(0.0, 0.0, 640.0, 480.0),
            canvas,
            screen,
            ScaleMode::PreserveAspect,
        );
        assert_eq!(r, Rect::new(0.25, 0.0, 0.5, 1.0));

        // Roundtrip: the normalized center of a mapped rect points back at the
        // canvas rect's center through pointer_to_canvas.
        let target = Rect::new(17.0, 166.0, 45.0, 60.0);
        let mapped = canvas_rect_to_screen(target, canvas, screen, ScaleMode::PreserveAspect);
        let back = pointer_to_canvas(canvas, mapped.center(), screen, ScaleMode::PreserveAspect)
            .expect("center should land inside the canvas");
        assert!((back.x - target.center().x).abs() < 1e-3);
        assert!((back.y - target.center().y).abs() < 1e-3);
    }

    #[test]
    fn preserve_aspect_is_stretch_when_aspect_matches() {
        // 4:3 canvas on a 4:3 screen: no bars, so it matches stretch.
        let canvas = vec2(640.0, 480.0);
        let p = pointer_to_canvas(
            canvas,
            vec2(0.5, 0.25),
            vec2(800.0, 600.0),
            ScaleMode::PreserveAspect,
        );
        assert_eq!(p, Some(vec2(320.0, 120.0)));
    }

    /// Object icons are blitted 1:1 at their authored size, not stretched to
    /// the slot: the wrench ships a 22x99 icon for a 1x3 (35x102) slot, so
    /// stretching it would make it ~60% too wide next to every other item.
    #[test]
    fn object_icons_draw_at_their_authored_pixel_size() {
        let slot = vec2(35.0, 102.0);
        let icon = vec2(22.0, 99.0);

        let at = vec2(15.0, 153.0);
        // Size is the icon's own; position is its centered placement in the slot.
        assert_eq!(
            drawn_rect(at, slot, icon, ImageKind::ObjectIcon),
            (at + centered_offset(slot, icon), icon)
        );
        assert_eq!(drawn_rect(at, slot, icon, ImageKind::Ui), (at, slot));
    }

    #[test]
    fn fitted_object_icons_letterbox_without_distorting_or_upscaling() {
        let slot = vec2(30.0, 50.0);
        let at = vec2(10.0, 15.0);
        assert!(ImageKind::ObjectIconFit.transparent_index_0());

        let (tall_at, tall_size) = drawn_rect(at, slot, vec2(34.0, 99.0), ImageKind::ObjectIconFit);
        assert_eq!(tall_at, at + vec2(6.0, 0.0));
        assert!((tall_size.x - 34.0 * 50.0 / 99.0).abs() < 1e-5);
        assert!((tall_size.y - 50.0).abs() < 1e-5);

        assert_eq!(
            drawn_rect(at, slot, vec2(16.0, 16.0), ImageKind::ObjectIconFit),
            (at + vec2(7.0, 17.0), vec2(16.0, 16.0)),
            "small icons should retain their authored pixels"
        );
    }

    /// ...and centered in the slot, so narrow art (the 22px wrench in a 35px
    /// cell) is not shoved against one separator with all its slack on the
    /// other side. Whole pixels only, and art wider than its slot keeps the
    /// top-left anchor instead of overhanging into the panel's chrome.
    #[test]
    fn object_icons_center_in_their_slot() {
        assert_eq!(
            centered_offset(vec2(35.0, 102.0), vec2(22.0, 99.0)),
            vec2(6.0, 1.0)
        );
        assert_eq!(
            centered_offset(vec2(35.0, 34.0), vec2(32.0, 32.0)),
            vec2(1.0, 1.0)
        );
        // No half-pixel origins: 35 - 34 = 1 floors to 0 rather than 0.5.
        assert_eq!(
            centered_offset(vec2(35.0, 102.0), vec2(34.0, 99.0)),
            vec2(0.0, 1.0)
        );
        // Art larger than its slot stays anchored top-left.
        assert_eq!(
            centered_offset(vec2(35.0, 34.0), vec2(66.0, 68.0)),
            vec2(0.0, 0.0)
        );
    }

    #[test]
    fn object_icon_marks_palette_index_zero_as_transparent() {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        canvas.image(Rect::new(0.0, 0.0, 640.0, 120.0), "invback.pcx");
        canvas.object_icon(Rect::new(0.0, 0.0, 32.0, 32.0), "passkey.pcx");

        assert!(matches!(
            canvas.elements.as_slice(),
            [
                UiElement::Image {
                    kind: ImageKind::Ui,
                    ..
                },
                UiElement::Image {
                    kind: ImageKind::ObjectIcon,
                    ..
                }
            ]
        ));
    }

    mod world_panel {
        use super::*;
        use cgmath::{Deg, Rotation3};

        /// A 4:3 panel 2m in front of the origin, facing back toward it.
        fn panel() -> WorldPanel {
            WorldPanel {
                center: vec3(0.0, 0.0, -2.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                size: vec2(2.0, 1.5),
            }
        }

        const CANVAS: Vector2<f32> = Vector2 { x: 640.0, y: 480.0 };

        fn assert_close(actual: Vector2<f32>, expected: Vector2<f32>) {
            assert!(
                (actual.x - expected.x).abs() < 0.01 && (actual.y - expected.y).abs() < 0.01,
                "expected {:?}, got {:?}",
                expected,
                actual
            );
        }

        #[test]
        fn a_ray_down_the_axis_hits_the_canvas_center() {
            let hit = ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0));
            assert_close(hit.expect("the ray should hit"), vec2(320.0, 240.0));
        }

        #[test]
        fn world_up_is_canvas_up() {
            // Aiming above center must land in the TOP half of the canvas
            // (canvas y grows downward, world y grows upward) - the flip is
            // exactly the kind of thing that silently inverts a menu.
            let hit = ray_to_canvas(
                CANVAS,
                &panel(),
                vec3(0.0, 0.375, 0.0),
                vec3(0.0, 0.0, -1.0),
            )
            .expect("the ray should hit");
            assert_close(hit, vec2(320.0, 120.0));
            assert!(hit.y < 240.0, "aiming up must map to the top of the canvas");
        }

        #[test]
        fn world_right_is_canvas_right() {
            let hit = ray_to_canvas(CANVAS, &panel(), vec3(0.5, 0.0, 0.0), vec3(0.0, 0.0, -1.0))
                .expect("the ray should hit");
            assert_close(hit, vec2(480.0, 240.0));
        }

        #[test]
        fn a_ray_past_the_edge_misses() {
            // Just outside the panel's 2m width.
            assert_eq!(
                ray_to_canvas(CANVAS, &panel(), vec3(1.01, 0.0, 0.0), vec3(0.0, 0.0, -1.0)),
                None
            );
            assert_eq!(
                ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.76, 0.0), vec3(0.0, 0.0, -1.0)),
                None
            );
        }

        #[test]
        fn the_corners_map_to_the_canvas_corners() {
            // Top-left in world terms (-x, +y) is canvas (0, 0).
            let hit = ray_to_canvas(CANVAS, &panel(), vec3(-1.0, 0.75, 0.0), vec3(0.0, 0.0, -1.0))
                .expect("the corner should hit");
            assert_close(hit, vec2(0.0, 0.0));
            let hit = ray_to_canvas(CANVAS, &panel(), vec3(1.0, -0.75, 0.0), vec3(0.0, 0.0, -1.0))
                .expect("the corner should hit");
            assert_close(hit, vec2(640.0, 480.0));
        }

        #[test]
        fn a_ray_pointing_away_misses() {
            // The panel is behind the ray, so there is no forward hit.
            assert_eq!(
                ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0)),
                None
            );
        }

        #[test]
        fn a_ray_parallel_to_the_panel_misses() {
            assert_eq!(
                ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0)),
                None
            );
        }

        #[test]
        fn an_angled_ray_lands_off_center() {
            // 45 degrees right of straight ahead, from the origin: at 2m the
            // hit is 2m to the right, well outside the 1m half-width.
            let direction = vec3(1.0, 0.0, -1.0).normalize();
            assert_eq!(ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), direction), None);
            // A gentler angle stays on the panel and lands right of center.
            let direction = vec3(0.25, 0.0, -1.0).normalize();
            let hit = ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), direction)
                .expect("a gentle angle should still hit");
            assert!(hit.x > 320.0, "a rightward angle must land right of center");
        }

        #[test]
        fn a_rotated_panel_still_maps_correctly() {
            // Yaw the panel 90 degrees so it faces +X, and place it to the
            // player's right. Pointing along +X must hit its center.
            let panel = WorldPanel {
                center: vec3(2.0, 0.0, 0.0),
                rotation: Quaternion::from_angle_y(Deg(-90.0)),
                size: vec2(2.0, 1.5),
            };
            let hit = ray_to_canvas(CANVAS, &panel, vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0))
                .expect("the rotated panel should be hit");
            assert_close(hit, vec2(320.0, 240.0));
        }

        #[test]
        fn the_transform_scales_to_the_panel_size() {
            let panel = panel();
            // The canvas is authored in 0..1 space, so the transform must take
            // the unit square to the panel's metres, centered on the panel.
            let corner = panel.transform() * cgmath::vec4(0.5, 0.5, 0.0, 1.0);
            assert!(corner.x.abs() - 1.0 < 1e-5, "half-width should be 1m");
            assert!(corner.y.abs() - 0.75 < 1e-5, "half-height should be 0.75m");
            assert!((corner.z + 2.0).abs() < 1e-5, "panel sits 2m ahead");
        }

        #[test]
        fn the_transform_preserves_the_panels_facing() {
            // The element path's own 180-degree rotation is about Z (in-plane),
            // so `transform` must not add a facing correction of its own - an
            // extra Y flip turns the panel away and it renders to nothing.
            let panel = panel();
            let right = panel.transform() * cgmath::vec4(0.5, 0.0, 0.0, 1.0);
            assert!(right.x > 0.0, "local +X must stay along world +X");
            assert!((panel.normal().z - 1.0).abs() < 1e-5);
        }
    }
}
