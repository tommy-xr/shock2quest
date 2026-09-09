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
    Font,
    assets::asset_cache::AssetCache,
    ellipsize, measure_text_width,
    scene::SceneObject,
    texture::{Texture, TextureOptions, TextureTrait},
};
use shipyard::EntityId;

use crate::vr_config::Handedness;

pub mod dev_params_panel;
pub mod entry_ramp;
mod frontend_menu;
mod frontend_pointer;
mod frontend_presentation;
mod frontend_sfx;
pub mod list_scroll;
mod panel_anchor;
mod pointer_visual;
pub mod world_dim;
#[cfg(test)]
pub use frontend_menu::resolve_flat_click;
pub use frontend_menu::{
    FrontendMenu, FrontendMenuItem, flat_pointer_state, hit_menu_item, resolve_click_at,
    resolve_menu_label, resolve_menu_labels, resolve_menu_rects,
};
#[cfg(test)]
pub use frontend_pointer::test_support;
pub use frontend_pointer::{
    FrontendPointerPass, FrontendRay, PointerEngagement, VR_TRIGGER_THRESHOLD,
    vr_frontend_pointer_pass, vr_pointer_pass,
};
pub use frontend_presentation::FrontendCanvasPresenter;
pub use frontend_sfx::FrontendSfx;
pub use panel_anchor::{FrontendPanelAnchor, PanelPlacement};
pub use pointer_visual::{PointerVisuals, pointer_beams};

/// Font name that resolves to the engine's compiled-in font rather than a
/// `.FON` asset.
///
/// It is the only font available when the game data is missing - every other
/// font in the game is loaded out of `intrface.crf` or the KPF archives - so
/// the missing-assets screen names this one. Ordinary UI keeps naming its real
/// font and is unaffected.
pub const BUILTIN_FONT: &str = "@builtin";

/// Retail MFDs use MAINAA with the cyan text palette (shkutils.cpp).
pub const MFD_FONT: &str = "@shock-mfd";

/// The font for a `UiElement::Text`, whichever kind it is.
///
/// Both presentations and the layout pass go through here, so the two cannot
/// disagree about which font measured the text and which font draws it.
pub(crate) fn resolve_font(asset_cache: &mut AssetCache, font: &str) -> Rc<Box<dyn engine::Font>> {
    if font == BUILTIN_FONT {
        return engine::shared_builtin_font();
    }
    if font == MFD_FONT {
        // Family mounts strip their prefix. The bare key resolves the canonical
        // fonts family; "fonts/mainaa.fon" instead names iface's stripped copy.
        return asset_cache.get_ext(
            &dark::importers::TINTED_FONT_IMPORTER,
            "mainaa.fon",
            &[0, 255, 190],
        );
    }
    asset_cache.get(&FONT_IMPORTER, font).clone()
}

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

    /// Half-open containment: a point on the far edge belongs to the *next*
    /// rect, not to both. [`Rect::contains`] is inclusive on all four edges,
    /// which makes abutting rects - a row of tabs, a grid of cells - overlap by
    /// a pixel and pick two winners.
    pub fn contains_half_open(&self, p: Vector2<f32>) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
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
    ///
    /// The one mapping under which text is *not* presentation-independent:
    /// bitmap glyphs have a single size, so screen-space text scales uniformly
    /// (by the vertical factor) while a world-space panel scales its text mesh
    /// into the placed rect on both axes. Every shipped presentation maps the
    /// canvas uniformly - flat scenes use `PreserveAspect`, and world panels
    /// are sized `canvas_px * constant` - so the two agree in practice.
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
    /// The panel's outward face normal: local +Z, pointing at the viewer the
    /// panel was oriented toward.
    pub fn normal(&self) -> Vector3<f32> {
        self.rotation.rotate_vector(vec3(0.0, 0.0, 1.0))
    }

    /// Root transform for [`UiCanvas::render_world_space`].
    ///
    /// The basis is honest - local +x the viewer's right, +y up, +Z at the
    /// viewer - so a raw element placed with this transform alone renders
    /// upright. The canvas path adds only `world_element_transform`'s
    /// canvas-y flip on top.
    pub fn transform(&self) -> Matrix4<f32> {
        Matrix4::from_translation(self.center)
            * Matrix4::from(self.rotation)
            * Matrix4::from_nonuniform_scale(self.size.x, self.size.y, 1.0)
    }
}

/// The VR menu hangs on a panel 2m ahead of the player at eye level, sized to
/// the canvas's 4:3 aspect so the art is not stretched.
///
/// "Eye level" is not the scene origin: VR runtimes add a head offset on top of
/// the camera position this scene returns, so a panel at y=0 hangs below the
/// view and is never seen. The panel is hung off the head's tracked position
/// ([`crate::input_context::Head::position`], which defaults to that same eye
/// height) rather than off the origin.
///
/// Live-tunable ([`crate::dev_params::FRONTEND_PANEL_DISTANCE`]); read it per
/// frame rather than latching it.
pub fn frontend_panel_distance() -> f32 {
    crate::dev_params::get(crate::dev_params::FRONTEND_PANEL_DISTANCE)
}
pub const FRONTEND_PANEL_SIZE: Vector2<f32> = Vector2 { x: 2.0, y: 1.5 };

/// Spacing between stacked canvas layers in world space, so the labels sort in
/// front of the backdrop instead of z-fighting it.
pub const VR_COMPONENT_Z_STEP: f32 = 0.001;

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

/// Where a canvas point sits in world space on `panel` - the exact inverse of
/// [`ray_to_canvas`].
///
/// Lets a caller that already hit-tested a ray place something *at the hit*
/// (the VR pointer's dot) from the hit-test's own answer, instead of
/// re-intersecting the ray and hoping the two agree.
pub fn canvas_to_panel_world(
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    point: Vector2<f32>,
) -> Vector3<f32> {
    // Normalize into the same centered unit square `transform` places, then let
    // the panel's own transform do the placing - rather than re-deriving its
    // basis here, where it could drift from what the canvas renders with.
    let u = point.x / canvas_size.x - 0.5;
    let v = 0.5 - point.y / canvas_size.y;
    let placed = panel.transform() * cgmath::vec4(u, v, 0.0, 1.0);
    vec3(placed.x, placed.y, placed.z)
}

pub fn canvas_rect_to_screen(
    rect: Rect,
    canvas_size: Vector2<f32>,
    screen_size: Vector2<f32>,
    mode: ScaleMode,
) -> Rect {
    let (scale, offset) = fit(canvas_size, screen_size, mode);
    // The renderer's own mapping, then normalized - so this describes exactly
    // where the screen presentation puts the rect, not a parallel derivation.
    let px = canvas_rect_to_screen_px(rect, scale, offset);
    Rect::new(
        px.x / screen_size.x,
        px.y / screen_size.y,
        px.w / screen_size.x,
        px.h / screen_size.y,
    )
}

/// Where a canvas rect lands on a world-space panel, in normalized panel
/// coordinates (0..1 per axis, origin top-left) - the world counterpart of
/// [`canvas_rect_to_screen`].
///
/// Derived from the transform the renderer actually uses, so it cannot claim a
/// placement the panel does not draw. The element path's only flip is the
/// canvas-y one (panel-local y grows up, canvas y grows down); undoing it is
/// the whole conversion.
pub fn canvas_rect_to_panel(rect: Rect, canvas_size: Vector2<f32>) -> Rect {
    let transform =
        world_element_transform(vec2(rect.x, rect.y), vec2(rect.w, rect.h), canvas_size, 0.0);
    // `quad::create` (and the normalized world text mesh) span the centered
    // unit square, so these are the element's own corners.
    let corner = |x: f32, y: f32| {
        let p = transform * cgmath::vec4(x, y, 0.0, 1.0);
        vec2(p.x + 0.5, 0.5 - p.y)
    };
    let top_left = corner(-0.5, -0.5);
    let bottom_right = corner(0.5, 0.5);
    Rect::new(
        top_left.x,
        top_left.y,
        bottom_right.x - top_left.x,
        bottom_right.y - top_left.y,
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
    /// A holographic grid: the SHODAN family grid tile repeated `tiles_x` x
    /// `tiles_y` times across the element's rect, with its black dropped and
    /// its lines tinted (see [`HOLOGRAM_TINT`]). Sized like [`Self::Ui`].
    Hologram { tiles_x: u8, tiles_y: u8 },
    /// One sub-rectangle of the art, stretched to the element's rect - the
    /// corners in normalized texture coordinates, `v` measured from the top of
    /// the bitmap. Built by [`UiCanvas::cropped_image`]; lets a panel wear a
    /// region of a shipped bitmap (a single row of the bio monitor, the ammo
    /// gauge's well) without a second, hand-cut copy of the art.
    Crop { u0: f32, v0: f32, u1: f32, v1: f32 },
}

/// The grid tile every hologram panel is drawn from (`shodan/s45.pcx`): a black
/// cell bounded by a bright cross at row/column 64 (a 1px peak with a few
/// texels of glow), with a faint dotted sub-grid and a DIM line along the wrap
/// edge at row/column 0. Its size is fixed at the shipped asset's 128x128 - a
/// mod that replaced it with a different resolution would need this updated.
const HOLOGRAM_TILE_PX: f32 = 128.0;

/// The hologram's line colour. The tile's own art is near-white; a cyan tint
/// is what makes it read as SHODAN's projection rather than as white chrome.
const HOLOGRAM_TINT: [u8; 3] = [90, 226, 255];

/// The tile bitmap for [`ImageKind::Hologram`].
pub const HOLOGRAM_TILE_TEXTURE: &str = "shodan/s45.pcx";

impl ImageKind {
    pub(crate) fn transparent_index_0(self) -> bool {
        matches!(self, Self::ObjectIcon | Self::ObjectIconFit)
    }

    /// The texture rectangle this art samples, in texture units, or `None` for
    /// the whole texture. Both presentations ask this one question, so a
    /// hologram cannot tile differently on a flat panel than on a VR quad.
    ///
    /// A hologram spans exactly `tiles` whole tiles, starting at the CENTER of
    /// the tile's bright cross. Every cell border therefore lands on that
    /// bright line - the outer edges included - at the same sub-texel phase, so
    /// the separators are evenly bright and the cells line up exactly with the
    /// item grid's pitch. (A span that is not a whole number of tiles drifts:
    /// the borders creep off the item grid and each one samples the line at a
    /// different point, which renders them at visibly different brightness.)
    pub(crate) fn uv_rect(self) -> Option<(Vector2<f32>, Vector2<f32>)> {
        match self {
            Self::Hologram { tiles_x, tiles_y } => {
                let first = (HOLOGRAM_TILE_PX / 2.0 + 0.5) / HOLOGRAM_TILE_PX;
                Some((
                    vec2(first, first),
                    vec2(first + tiles_x as f32, first + tiles_y as f32),
                ))
            }
            Self::Crop { u0, v0, u1, v1 } => Some((vec2(u0, v0), vec2(u1, v1))),
            _ => None,
        }
    }
}

/// One element after layout: where it goes, how opaque it is, and what to
/// draw there. Produced by [`UiCanvas::layout`] and consumed by every
/// presentation.
///
/// `rect` is in canvas pixels and is final - for text it is the *glyph box*
/// (alignment already resolved, ellipsis already applied, height = font size),
/// not the authored widget box. A presentation's only job is to map this
/// rectangle into its own space.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedElement {
    pub rect: Rect,
    pub alpha: f32,
    pub content: PlacedContent,
}

/// What a [`PlacedElement`] draws. Deliberately smaller than [`UiElement`]:
/// buttons have collapsed into their resolved art, and alignment/fit options
/// are gone because layout has already applied them.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacedContent {
    Image {
        texture: String,
        kind: ImageKind,
    },
    Bar {
        texture: String,
        fill: f32,
    },
    /// Ellipsized, alignment-resolved text. There is no separate font size:
    /// the placed `rect` IS the glyph box, so its height is the line height.
    Text {
        text: String,
        font: String,
    },
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
        /// ellipsis instead of spilling over neighbouring widgets. Applied in
        /// the shared layout pass, so every presentation shortens the same
        /// text the same way (see [`UiCanvas::text_native_fit`]).
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

    /// Draw the `source` region of `texture` - in the art's own texels, with
    /// `art_size` its authored pixel size - stretched to `rect`.
    pub fn cropped_image(
        &mut self,
        rect: Rect,
        texture: &str,
        source: Rect,
        art_size: Vector2<f32>,
    ) -> &mut Self {
        self.elements.push(UiElement::Image {
            position: vec2(rect.x, rect.y),
            size: vec2(rect.w, rect.h),
            texture: texture.to_owned(),
            alpha: 1.0,
            kind: ImageKind::Crop {
                u0: source.x / art_size.x,
                v0: source.y / art_size.y,
                u1: (source.x + source.w) / art_size.x,
                v1: (source.y + source.h) / art_size.y,
            },
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

    /// Resolve every element to a concrete canvas-pixel rectangle plus the
    /// content to draw in it - the **single** layout pass.
    ///
    /// Everything that decides *where* or *what* happens here: alignment, text
    /// measurement, ellipsizing, and object-icon sizing. Presentations
    /// (screen space, world-space panel) are affine mappers over the result
    /// and make no placement decisions of their own, so they cannot drift
    /// apart the way three hand-written emit paths did.
    pub fn layout(&self, asset_cache: &mut AssetCache) -> Vec<PlacedElement> {
        self.layout_with_pointer(asset_cache, None)
    }

    /// [`layout`](Self::layout) with a pointer in canvas pixels, so buttons
    /// resolve their hover art. Hover is content, not placement: a hover
    /// texture can be a different size, and the swap must happen before the
    /// icon-sizing rules run.
    pub fn layout_with_pointer(
        &self,
        asset_cache: &mut AssetCache,
        pointer: Option<Vector2<f32>>,
    ) -> Vec<PlacedElement> {
        let mut placed = Vec::with_capacity(self.elements.len());
        for element in &self.elements {
            placed.push(match element {
                UiElement::Image {
                    position,
                    size,
                    texture,
                    alpha,
                    kind,
                } => place_image(asset_cache, *position, *size, texture, *alpha, *kind),
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
                    place_image(asset_cache, *position, *size, texture, *alpha, *kind)
                }
                UiElement::Bar {
                    position,
                    size,
                    texture,
                    fill,
                    alpha,
                } => PlacedElement {
                    rect: Rect::new(position.x, position.y, size.x, size.y),
                    alpha: *alpha,
                    content: PlacedContent::Bar {
                        texture: texture.clone(),
                        fill: *fill,
                    },
                },
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
                    let font_obj = resolve_font(asset_cache, font);
                    place_text(
                        &**font_obj,
                        Rect::new(position.x, position.y, size.x, size.y),
                        text,
                        font,
                        *font_size,
                        *h,
                        *v,
                        *alpha,
                        *fit_to_rect,
                    )
                }
            });
        }
        placed
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
        let placed = self.layout_with_pointer(asset_cache, pointer);
        let mut objects = Vec::with_capacity(placed.len());
        for element in &placed {
            let rect = canvas_rect_to_screen_px(element.rect, scale, offset);
            objects.push(present_screen(asset_cache, element, rect));
        }
        objects
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
        let placed = self.layout_with_pointer(asset_cache, pointer);
        let mut objects = Vec::with_capacity(placed.len());
        let layers = overlap_layers(placed.iter().map(|element| element.rect));
        for (element, layer) in placed.iter().zip(layers) {
            let alpha = force_alpha.unwrap_or(element.alpha);
            let mut object = present_world(asset_cache, element, alpha, self.size);
            // Only overlapping art needs separation. Stepping every element
            // forward gave adjacent backdrop crops different perspective
            // scales in VR, breaking borders that join exactly on screen.
            object.set_transform(
                root_transform
                    * Matrix4::from_translation(vec3(0.0, 0.0, component_z_step * layer as f32)),
            );
            objects.push(object);
        }
        objects
    }
}

/// Preserve painter order where rectangles overlap, keeping adjoining pieces
/// on the same plane. Input rectangles are the shared, fully resolved layout.
fn overlap_layers(rects: impl IntoIterator<Item = Rect>) -> Vec<usize> {
    let mut previous: Vec<(Rect, usize)> = Vec::new();
    for rect in rects {
        let layer = previous
            .iter()
            .filter(|(other, _)| {
                rect.x < other.x + other.w
                    && other.x < rect.x + rect.w
                    && rect.y < other.y + other.h
                    && other.y < rect.y + rect.h
            })
            .map(|(_, layer)| layer + 1)
            .max()
            .unwrap_or(0);
        previous.push((rect, layer));
    }
    previous.into_iter().map(|(_, layer)| layer).collect()
}

impl UiCanvas<()> {
    pub fn new(size: Vector2<f32>) -> Self {
        Self::with_events(size)
    }
}

/// How a `kind`'s art is sampled. Shared so layout and the presenters key the
/// asset cache the same way.
fn texture_options(kind: ImageKind) -> TextureOptions {
    let hologram = matches!(kind, ImageKind::Hologram { .. });
    TextureOptions {
        // A hologram repeats one tile across its cells, so it - and only it -
        // needs the sampler to wrap.
        wrap: hologram,
        transparent_index_0: kind.transparent_index_0(),
        luminance_alpha_tint: hologram.then_some(HOLOGRAM_TINT),
        // The grid's lines are a texel wide and are drawn well under their
        // authored size, at a distance the VR viewer changes at will; without
        // mipmaps they alias into a swimming, unevenly-bright grid.
        filter: if hologram {
            engine::texture::TextureFilter::LinearMipmap
        } else {
            engine::texture::TextureFilter::Linear
        },
        ..Default::default()
    }
}

/// Lay out one piece of art: its slot rect, unless it is object-icon art,
/// which draws at its own authored pixel size centered in the slot (or
/// uniformly downscaled to fit it, for [`ImageKind::ObjectIconFit`]).
fn place_image(
    asset_cache: &mut AssetCache,
    position: Vector2<f32>,
    size: Vector2<f32>,
    texture: &str,
    alpha: f32,
    kind: ImageKind,
) -> PlacedElement {
    // Upgrade object icons, whose art is self-contained. Ordinary UI bitmaps
    // may contain baked labels that HD replacements omit (MAP/RESEARCH/etc.),
    // so keep their requested encoding until their labels are drawn separately.
    let texture = if kind.transparent_index_0() {
        dark::util::resolve_object_icon_name(asset_cache, texture)
            .unwrap_or_else(|| texture.to_owned())
    } else {
        texture.to_owned()
    };
    let loaded = asset_cache.get_ext(&TEXTURE_IMPORTER, &texture, &texture_options(kind));
    // Replacement icons have more texels, not a larger inventory footprint.
    // Keep the classic bitmap's authored size while drawing the upgraded art.
    let authored = if matches!(kind, ImageKind::ObjectIcon | ImageKind::ObjectIconFit) {
        let original = std::path::Path::new(&texture).with_extension("pcx");
        asset_cache
            .get_ext_opt(
                &TEXTURE_IMPORTER,
                original.to_str().unwrap(),
                &texture_options(kind),
            )
            .map(|original| texture_px(&original))
            .unwrap_or_else(|| texture_px(&loaded))
    } else {
        texture_px(&loaded)
    };
    let (position, size) = drawn_rect(position, size, authored, kind);
    PlacedElement {
        rect: Rect::new(position.x, position.y, size.x, size.y),
        alpha,
        content: PlacedContent::Image {
            texture: texture.to_owned(),
            kind,
        },
    }
}

/// Lay out one text element inside its authored widget `rect`: pick the size,
/// shorten it to fit, measure it, and resolve both alignments - the whole of
/// "where does this string go", in canvas pixels.
///
/// Pure (it needs only the font's metrics), which is what lets the parity test
/// exercise it over many alignments and strings without a GPU.
#[allow(clippy::too_many_arguments)]
fn place_text(
    font: &dyn Font,
    rect: Rect,
    text: &str,
    font_name: &str,
    font_size: f32,
    h: HAlign,
    v: VAlign,
    alpha: f32,
    fit_to_rect: bool,
) -> PlacedElement {
    // `font_size <= 0` renders at the font's native pixel height, so Dark
    // `.FON` bitmap fonts draw at their authored size (the way the original
    // engine does) instead of an ad-hoc scale.
    let font_size = if font_size > 0.0 {
        font_size
    } else {
        font.base_height()
    };
    // Ellipsizing and alignment are measured in canvas pixels, so they are
    // resolution-independent and identical in every presentation.
    let text = if fit_to_rect {
        ellipsize(font, text, font_size, rect.w)
    } else {
        text.to_owned()
    };
    let width = measure_text_width(font, &text, font_size);
    let x = match h {
        HAlign::Left => rect.x,
        HAlign::Center => rect.x + (rect.w - width) / 2.0,
        HAlign::Right => rect.x + rect.w - width,
    };
    let y = match v {
        VAlign::Top => rect.y,
        VAlign::Middle => rect.y + (rect.h - font_size) / 2.0,
        VAlign::Bottom => rect.y + rect.h - font_size,
    };
    PlacedElement {
        // The glyph box, not the authored widget box: its height IS the font
        // size and its width the measured text width, so "draw this text in
        // this rect" means the same thing to every presentation.
        rect: Rect::new(x, y, width, font_size),
        alpha,
        content: PlacedContent::Text {
            text,
            font: font_name.to_owned(),
        },
    }
}

/// Canvas rect -> screen pixels, under a canvas->screen `fit`.
fn canvas_rect_to_screen_px(rect: Rect, scale: Vector2<f32>, offset: Vector2<f32>) -> Rect {
    Rect::new(
        rect.x * scale.x + offset.x,
        rect.y * scale.y + offset.y,
        rect.w * scale.x,
        rect.h * scale.y,
    )
}

/// Screen-space presentation of one placed element, already mapped to screen
/// pixels. Chooses a material; it never moves anything.
fn present_screen(
    asset_cache: &mut AssetCache,
    element: &PlacedElement,
    rect: Rect,
) -> SceneObject {
    match &element.content {
        PlacedContent::Image { texture, kind } => {
            let texture = asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options(*kind));
            match kind.uv_rect() {
                Some((uv_min, uv_max)) => SceneObject::screen_space_quad_uv(
                    texture.clone() as Rc<dyn TextureTrait>,
                    vec2(rect.x, rect.y),
                    vec2(rect.w, rect.h),
                    element.alpha,
                    uv_min,
                    uv_max,
                ),
                None => SceneObject::screen_space_quad2(
                    texture.clone() as Rc<dyn TextureTrait>,
                    vec2(rect.x, rect.y),
                    vec2(rect.w, rect.h),
                    element.alpha,
                ),
            }
        }
        PlacedContent::Bar { texture, fill } => {
            let texture =
                asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options(ImageKind::Ui));
            SceneObject::screen_space_clipped_quad(
                texture.clone() as Rc<dyn TextureTrait>,
                vec2(rect.x, rect.y),
                vec2(rect.w, rect.h),
                *fill,
            )
        }
        PlacedContent::Text { text, font, .. } => {
            let font_obj = resolve_font(asset_cache, font);
            // `screen_space_text` anchors on the glyph box's top-left and takes
            // the line height as its size - which is exactly what the placed
            // rect is, so there is nothing to adjust. The rect's *width* is
            // measured, not imposed: a non-uniform canvas->screen scale
            // (`ScaleMode::Stretch` on a non-4:3 screen) scales glyphs by the
            // vertical factor only, since bitmap text has one size.
            SceneObject::screen_space_text(text, font_obj, rect.h, element.alpha, rect.x, rect.y)
        }
    }
}

/// World-space (panel) presentation of one placed element.
///
/// Every content kind is placed by the same [`world_element_transform`] call
/// on the same rect - text included, because `world_space_text` normalizes its
/// glyph box to the centered unit square just like `quad::create`. There is no
/// per-kind placement left to disagree about (there used to be: text carried
/// its own rotation, its own half-line anchor fudge, and a fixed font size
/// that ignored the layout entirely).
fn present_world(
    asset_cache: &mut AssetCache,
    element: &PlacedElement,
    alpha: f32,
    canvas_size: Vector2<f32>,
) -> SceneObject {
    let rect = element.rect;
    let mut object = match &element.content {
        PlacedContent::Image { texture, kind } => {
            let texture = asset_cache
                .get_ext(&TEXTURE_IMPORTER, texture, &texture_options(*kind))
                .clone();
            let material = engine::scene::basic_material::create(
                texture as Rc<dyn TextureTrait>,
                1.0,
                1.0 - alpha,
            );
            match kind.uv_rect() {
                Some((uv_min, uv_max)) => SceneObject::new(
                    material,
                    Box::new(engine::scene::quad::create_with_uv(uv_min, uv_max)),
                ),
                None => SceneObject::new(material, Box::new(engine::scene::quad::create())),
            }
        }
        PlacedContent::Bar { texture, fill } => {
            let texture =
                asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options(ImageKind::Ui));
            let material = engine::scene::clipped_screen_material::create(
                texture.clone() as Rc<dyn TextureTrait>,
                *fill,
            );
            SceneObject::new(material, Box::new(engine::scene::quad::create()))
        }
        PlacedContent::Text { text, font, .. } => {
            let font_obj = resolve_font(asset_cache, font);
            SceneObject::world_space_text(text, font_obj, (1.0 - alpha).clamp(0.0, 1.0))
        }
    };
    object.set_local_transform(world_element_transform(
        vec2(rect.x, rect.y),
        vec2(rect.w, rect.h),
        canvas_size,
        0.0,
    ));
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
        ImageKind::Ui | ImageKind::Hologram { .. } | ImageKind::Crop { .. } => (position, size),
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

/// Canvas y grows downward while panel-local y grows up, so the element must
/// be y-flipped into place. The flip is a 180-degree rotation about **X** - a
/// proper rotation, not a mirror - so triangle winding is preserved; the cost
/// is that the quad shows the viewer its local -Z face, which the untextured
/// two-sided UI materials render identically. Canvas x maps straight to
/// panel-local +x (the viewer's right), with no compensation anywhere.
fn world_element_transform(
    position: Vector2<f32>,
    size: Vector2<f32>,
    canvas_size: Vector2<f32>,
    z: f32,
) -> Matrix4<f32> {
    let position = vec2(position.x / canvas_size.x, position.y / canvas_size.y);
    let size = vec2(size.x / canvas_size.x, size.y / canvas_size.y);
    Matrix4::from_angle_x(Deg(180.0))
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
    fn adjacent_background_pieces_share_depth_but_overlays_keep_painter_order() {
        let layers = overlap_layers([
            Rect::new(0.0, 0.0, 100.0, 20.0),
            Rect::new(100.0, 0.0, 20.0, 5.0),
            Rect::new(100.0, 5.0, 20.0, 15.0),
            Rect::new(10.0, 5.0, 20.0, 10.0),
            Rect::new(12.0, 6.0, 5.0, 5.0),
            Rect::new(105.0, 6.0, 5.0, 5.0),
        ]);
        assert_eq!(layers, [0, 0, 0, 1, 2, 1]);
    }

    #[test]
    fn cropped_art_keeps_its_destination_rect_and_authored_texel_bounds() {
        let mut canvas = UiCanvas::new(vec2(90.0, 44.0));
        canvas.cropped_image(
            Rect::new(0.0, 0.0, 90.0, 44.0),
            "AMMOFULL.PCX",
            Rect::new(168.0, 14.0, 90.0, 44.0),
            vec2(260.0, 64.0),
        );
        let UiElement::Image {
            position,
            size,
            kind,
            ..
        } = &canvas.elements()[0]
        else {
            panic!("expected image")
        };
        assert_eq!(
            drawn_rect(*position, *size, vec2(260.0, 64.0), *kind),
            (*position, *size)
        );
        assert_eq!(
            kind.uv_rect(),
            Some((
                vec2(168.0 / 260.0, 14.0 / 64.0),
                vec2(258.0 / 260.0, 58.0 / 64.0)
            ))
        );
    }

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

    /// One cell = one whole tile, bounded by the tile's bright cross - and
    /// every border, outer edges included, samples that line at the SAME
    /// sub-texel phase. A span that is not a whole number of tiles (an outward
    /// margin, say) fails this: the phase drifts across the panel and the
    /// separators render at visibly different brightness.
    #[test]
    fn a_hologram_puts_every_cell_border_on_the_bright_line() {
        const TILES: u8 = 4;
        let (uv_min, uv_max) = ImageKind::Hologram {
            tiles_x: TILES,
            tiles_y: TILES,
        }
        .uv_rect()
        .expect("a hologram samples a sub-rectangle");

        let bright_texel_center = 64.5 / 128.0;
        for border in 0..=TILES {
            let u = uv_min.x + (uv_max.x - uv_min.x) * (border as f32 / TILES as f32);
            assert!(
                (u.fract() - bright_texel_center).abs() < 1e-6,
                "border {border} samples texel {} of its tile, not the bright cross",
                u.fract() * 128.0
            );
        }
        assert_eq!(uv_max.x - uv_min.x, TILES as f32, "a whole number of tiles");
        assert_eq!(uv_min.x, uv_min.y, "square cells sample squarely");

        assert_eq!(ImageKind::Ui.uv_rect(), None);
        assert_eq!(ImageKind::ObjectIcon.uv_rect(), None);
    }

    /// A hologram tiles (so its sampler must wrap) and drops its black (so the
    /// world shows through between the lines).
    #[test]
    fn hologram_art_tiles_and_drops_its_black() {
        let options = texture_options(ImageKind::Hologram {
            tiles_x: 4,
            tiles_y: 4,
        });
        assert!(options.wrap);
        assert_eq!(options.luminance_alpha_tint, Some(HOLOGRAM_TINT));
        assert!(matches!(
            options.filter,
            engine::texture::TextureFilter::LinearMipmap
        ));
        assert!(!options.transparent_index_0);

        let ui = texture_options(ImageKind::Ui);
        assert!(!ui.wrap);
        assert_eq!(ui.luminance_alpha_tint, None);
    }

    /// A hologram fills its slot like ordinary UI art - the tile scales to the
    /// cells, it is not blitted at its authored 128px.
    #[test]
    fn a_hologram_stretches_to_its_rect() {
        let at = vec2(15.0, 8.0);
        let size = vec2(140.0, 136.0);
        assert_eq!(
            drawn_rect(
                at,
                size,
                vec2(128.0, 128.0),
                ImageKind::Hologram {
                    tiles_x: 4,
                    tiles_y: 4
                }
            ),
            (at, size)
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

    /// The two presentations must place identical content identically.
    ///
    /// Screen space and a world-space panel are both "map the canvas onto a
    /// 0..1 box"; the only thing that could ever make them disagree is a
    /// placement decision taken *inside* a presentation. Since
    /// [`UiCanvas::layout`] now owns every such decision, these assertions are
    /// a check on that contract - if someone re-introduces alignment,
    /// measurement or an anchor fudge in one mapper, this fails.
    mod parity {
        use super::*;

        /// Fixed-metrics stub font: every glyph is 4 wide at a base height of
        /// 10, except `?`, which the font does not have.
        struct StubFont;

        impl Font for StubFont {
            fn get_texture(&self) -> Rc<dyn TextureTrait> {
                unreachable!("layout does not touch the texture")
            }
            fn get_character_info(&self, c: char) -> Option<engine::FontCharacterInfo> {
                if c == '?' {
                    return None;
                }
                Some(engine::FontCharacterInfo {
                    min_uv_x: 0.0,
                    min_uv_y: 0.0,
                    max_uv_x: 1.0,
                    max_uv_y: 1.0,
                    advance: 4.0,
                })
            }
            fn base_height(&self) -> f32 {
                10.0
            }
            fn get_half_pixel(&self) -> f32 {
                0.0
            }
        }

        const CANVAS: Vector2<f32> = Vector2 { x: 640.0, y: 480.0 };

        fn assert_same_rect(what: &str, screen: Rect, panel: Rect) {
            let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
            assert!(
                close(screen.x, panel.x)
                    && close(screen.y, panel.y)
                    && close(screen.w, panel.w)
                    && close(screen.h, panel.h),
                "{what}: screen {screen:?} != panel {panel:?}"
            );
        }

        /// Every placed rect must normalize the same way in both
        /// presentations, on any screen shape.
        /// Every placed rect must normalize the same way in both
        /// presentations. `uniform_only` restricts the screen mappings to
        /// uniform ones: a bitmap glyph has a single size, so under a
        /// non-uniform canvas scale screen text keeps its aspect while a world
        /// panel stretches its text mesh into the rect (see [`ScaleMode`]).
        /// Art has no such constraint and must agree under any mapping.
        fn assert_parity_with(what: &str, placed: &PlacedElement, uniform_only: bool) {
            let panel = canvas_rect_to_panel(placed.rect, CANVAS);
            if !uniform_only {
                // Stretch fills the target, so normalized screen coordinates
                // are canvas coordinates - comparable on any screen shape.
                for screen in [
                    vec2(640.0, 480.0),
                    vec2(1920.0, 1080.0),
                    vec2(800.0, 1200.0),
                ] {
                    assert_same_rect(
                        what,
                        canvas_rect_to_screen(placed.rect, CANVAS, screen, ScaleMode::Stretch),
                        panel,
                    );
                }
            }
            // Aspect-preserving on screens of the canvas's own aspect: no bars,
            // uniform scale - the mapping every shipped presentation uses.
            for screen in [vec2(640.0, 480.0), vec2(1280.0, 960.0), vec2(320.0, 240.0)] {
                assert_same_rect(
                    what,
                    canvas_rect_to_screen(placed.rect, CANVAS, screen, ScaleMode::PreserveAspect),
                    panel,
                );
            }
        }

        fn assert_parity(what: &str, placed: &PlacedElement) {
            assert_parity_with(what, placed, false);
        }

        fn text(
            text: &str,
            widget: Rect,
            h: HAlign,
            v: VAlign,
            fit_to_rect: bool,
        ) -> PlacedElement {
            place_text(
                &StubFont,
                widget,
                text,
                "stub.fon",
                0.0,
                h,
                v,
                1.0,
                fit_to_rect,
            )
        }

        #[test]
        fn text_lands_in_the_same_place_in_both_presentations() {
            let widget = Rect::new(100.0, 60.0, 240.0, 40.0);
            for h in [HAlign::Left, HAlign::Center, HAlign::Right] {
                for v in [VAlign::Top, VAlign::Middle, VAlign::Bottom] {
                    for body in [
                        "",
                        "a",
                        "save1",
                        // Multi-byte: ellipsizing must not split a char, and
                        // both presentations must agree about the result.
                        "\u{e9}l\u{e9}phant \u{2014} caf\u{e9}",
                        "a very long save name that cannot possibly fit",
                    ] {
                        for fit in [false, true] {
                            let placed = text(body, widget, h, v, fit);
                            assert_parity_with(
                                &format!("{body:?} {h:?} {v:?} fit={fit}"),
                                &placed,
                                true,
                            );
                        }
                    }
                }
            }
        }

        #[test]
        fn images_and_bars_land_in_the_same_place_in_both_presentations() {
            for rect in [
                Rect::new(0.0, 0.0, 640.0, 480.0),
                Rect::new(17.0, 166.0, 45.0, 60.0),
                Rect::new(639.0, 479.0, 1.0, 1.0),
            ] {
                assert_parity(
                    "image",
                    &PlacedElement {
                        rect,
                        alpha: 1.0,
                        content: PlacedContent::Image {
                            texture: "key10.pcx".to_owned(),
                            kind: ImageKind::Ui,
                        },
                    },
                );
                assert_parity(
                    "bar",
                    &PlacedElement {
                        rect,
                        alpha: 1.0,
                        content: PlacedContent::Bar {
                            texture: "bar.pcx".to_owned(),
                            fill: 0.5,
                        },
                    },
                );
            }
        }

        /// Text is placed by rect like everything else, so a label and the art
        /// behind it cannot drift apart in one presentation only.
        #[test]
        fn a_text_rect_maps_exactly_like_an_image_rect() {
            let placed = text(
                "save1",
                Rect::new(20.0, 30.0, 200.0, 24.0),
                HAlign::Center,
                VAlign::Middle,
                false,
            );
            let image = PlacedElement {
                rect: placed.rect,
                alpha: 1.0,
                content: PlacedContent::Image {
                    texture: "x.pcx".to_owned(),
                    kind: ImageKind::Ui,
                },
            };
            assert_eq!(
                canvas_rect_to_panel(placed.rect, CANVAS),
                canvas_rect_to_panel(image.rect, CANVAS)
            );
        }

        /// Ellipsizing is a layout decision, so it happens once and both
        /// presentations receive the already-shortened string. It used to be
        /// done inside the screen mapper, which is why world-space text
        /// overflowed its widget.
        #[test]
        fn ellipsize_applies_in_layout_not_in_a_presentation() {
            // 4px per glyph at native size 10: a 40px box fits 10 glyphs, and
            // the ellipsis costs 3 of them.
            let widget = Rect::new(0.0, 0.0, 40.0, 20.0);
            let long = "abcdefghijklmnop";
            let PlacedContent::Text { text: fitted, .. } =
                &text(long, widget, HAlign::Left, VAlign::Top, true).content
            else {
                panic!("expected text");
            };
            assert_eq!(fitted, "abcdefg...");
            assert!(measure_text_width(&StubFont, fitted, 10.0) <= widget.w);

            // Without the option the string is untouched (and overflows) - in
            // both presentations, identically.
            let PlacedContent::Text { text: unfitted, .. } =
                &text(long, widget, HAlign::Left, VAlign::Top, false).content
            else {
                panic!("expected text");
            };
            assert_eq!(unfitted, long);
        }

        /// A multi-byte string must be cut on a character boundary (a byte cut
        /// would panic) and stay inside its box.
        #[test]
        fn ellipsize_respects_multi_byte_characters() {
            let widget = Rect::new(0.0, 0.0, 40.0, 20.0);
            let PlacedContent::Text { text: fitted, .. } = &text(
                "\u{e9}l\u{e9}phant \u{2014} caf\u{e9}",
                widget,
                HAlign::Left,
                VAlign::Top,
                true,
            )
            .content
            else {
                panic!("expected text");
            };
            assert!(fitted.ends_with("..."));
            assert!(measure_text_width(&StubFont, fitted, 10.0) <= widget.w);
        }

        /// The alignment cases themselves, in canvas pixels - the values both
        /// presentations then agree on.
        #[test]
        fn alignment_resolves_against_the_widget_box() {
            let widget = Rect::new(100.0, 60.0, 240.0, 40.0);
            // "abcd" is 4 glyphs * 4px = 16px wide, 10px tall (native).
            let left = text("abcd", widget, HAlign::Left, VAlign::Top, false).rect;
            assert_eq!(left, Rect::new(100.0, 60.0, 16.0, 10.0));

            let center = text("abcd", widget, HAlign::Center, VAlign::Middle, false).rect;
            assert_eq!(center, Rect::new(100.0 + 112.0, 60.0 + 15.0, 16.0, 10.0));

            let right = text("abcd", widget, HAlign::Right, VAlign::Bottom, false).rect;
            assert_eq!(right, Rect::new(100.0 + 224.0, 60.0 + 30.0, 16.0, 10.0));
        }
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
            let hit = ray_to_canvas(
                CANVAS,
                &panel(),
                vec3(-1.0, 0.75, 0.0),
                vec3(0.0, 0.0, -1.0),
            )
            .expect("the corner should hit");
            assert_close(hit, vec2(0.0, 0.0));
            let hit = ray_to_canvas(
                CANVAS,
                &panel(),
                vec3(1.0, -0.75, 0.0),
                vec3(0.0, 0.0, -1.0),
            )
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
            assert_eq!(
                ray_to_canvas(CANVAS, &panel(), vec3(0.0, 0.0, 0.0), direction),
                None
            );
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
            // `transform` must not add a facing correction of its own: the
            // basis is honest (+Z at the viewer), and an extra Y flip would
            // turn the panel away and mirror it.
            let panel = panel();
            let right = panel.transform() * cgmath::vec4(0.5, 0.0, 0.0, 1.0);
            assert!(right.x > 0.0, "local +X must stay along world +X");
            assert!((panel.normal().z - 1.0).abs() < 1e-5);
        }
    }
}
