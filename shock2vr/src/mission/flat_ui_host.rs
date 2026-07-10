//! Flat-mode MFD panel host (projects/flat-ui.md §5.2, PR 2).
//!
//! The flat presentation of the shared `Gui` layer: where VR shows every
//! panel as an always-on world quad (`GuiManager` + `ProxyGuiScript`), the
//! original flat game *opens* an object-bound MFD overlay on frob and drives
//! it with the mouse cursor. This host holds that flat-only state:
//!
//! - **Open**: `Effect::OpenPanel { entity }` (emitted by `GuiScript` on
//!   Frob) binds the panel to one world object - the original's single
//!   `gOverlayObj` binding.
//! - **Render**: the panel's `Effect::SetUI` component list is intercepted
//!   and drawn onto the shared 640x480 [`UiCanvas`] at the original left-MFD
//!   anchor `(2, 124)` (shkmfddm.h), after the flat HUD, plus a host-drawn
//!   close button and the `CURSOR.PCX` pointer.
//! - **Input**: each frame the normalized 2D pointer is mapped through
//!   [`pointer_to_canvas`] into panel-local normalized coordinates and sent
//!   to the panel entity as `MessagePayload::GUIHover` - the exact contract
//!   the VR hand ray synthesizes (`virtual_hand.rs` / `ProxyGuiScript`), so
//!   every existing panel (keypad, container, elevator, ...) works unchanged.
//! - **Close**: the host close button, LMB on the bare 3D view (the
//!   original's exit gesture), or walking away from the bound object (the
//!   original's per-overlay `distance` auto-close).

use cgmath::{InnerSpace, Vector2, point2};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntitiesView, EntityId, Get, UniqueView, View, World};

use crate::{
    gui::GuiComponentRenderInfo,
    input_context::Pointer2D,
    mission::PlayerInfo,
    scripts::{Message, MessagePayload},
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
    vr_config::Handedness,
};

/// The shared 640x480 virtual canvas the flat HUD renders on.
const CANVAS_SIZE: Vector2<f32> = Vector2::new(640.0, 480.0);

/// The original left MFD slot anchor for world-object panels (keypad,
/// container, ...) on the 640x480 canvas (`shkmfddm.h`: `(2, 124, 188x300)`).
/// The right slot `(450, 124)` is reserved for later character panels.
const LEFT_MFD_ANCHOR: Vector2<f32> = Vector2::new(2.0, 124.0);

/// Walk-away auto-close distance (world units; dark units / SCALE_FACTOR).
/// ~10 feet - past normal frob range, so a panel opened up close survives
/// small repositioning but closes when the player leaves the object.
const PANEL_AUTO_CLOSE_DISTANCE: f32 = 4.0;

/// Host-drawn close button, panel-local: the original keypad overlay's
/// CloseOff/CloseOn gadget at (163, 8, 20x21) in the 188-wide panel,
/// generalized to hug the panel's top-right corner.
const CLOSE_BUTTON_SIZE: Vector2<f32> = Vector2::new(20.0, 21.0);
const CLOSE_BUTTON_MARGIN: Vector2<f32> = Vector2::new(25.0, 8.0);

/// `CURSOR.PCX` native size.
const CURSOR_SIZE: Vector2<f32> = Vector2::new(12.0, 16.0);

/// Flat-mode MFD panel state: which world object (if any) has its panel
/// open, the panel's latest components (from `Effect::SetUI`), and the
/// cursor/pointer bookkeeping needed to render and hit-test them.
pub struct FlatUiHost {
    active_panel: Option<EntityId>,
    /// Panel size in panel-local pixels (from `SetUI.world_size`); `None`
    /// until the panel's first `SetUI` arrives (the frame after opening).
    panel_size_px: Option<Vector2<f32>>,
    /// Latest `SetUI` components for the active panel (normalized panel
    /// coordinates, as `GuiScript` emits them).
    components: Vec<GuiComponentRenderInfo>,
    /// Pointer position on the 640x480 canvas (None: no pointer / letterbox).
    cursor_canvas: Option<Vector2<f32>>,
    hover_close: bool,
    last_pointer_pressed: bool,
    /// Last known render-target size, for pointer->canvas letterbox mapping
    /// (updated every rendered frame; 4:3 default until the first render).
    screen_size: Vector2<f32>,
}

impl FlatUiHost {
    pub fn new() -> FlatUiHost {
        FlatUiHost {
            active_panel: None,
            panel_size_px: None,
            components: Vec::new(),
            cursor_canvas: None,
            hover_close: false,
            last_pointer_pressed: false,
            screen_size: CANVAS_SIZE,
        }
    }

    pub fn active_panel(&self) -> Option<EntityId> {
        self.active_panel
    }

    /// Bind the MFD to `entity` (the original's `gOverlayObj`). Opening a
    /// second panel replaces the first - one panel per (left) slot.
    pub fn open(&mut self, entity: EntityId) {
        if self.active_panel != Some(entity) {
            self.components.clear();
            self.panel_size_px = None;
        }
        self.active_panel = Some(entity);
    }

    pub fn close(&mut self) {
        self.active_panel = None;
        self.panel_size_px = None;
        self.components.clear();
        self.hover_close = false;
    }

    /// Observe an `Effect::SetUI`: stash the component list if it belongs to
    /// the active panel (the VR world-quad path is untouched by this).
    pub fn on_set_ui(
        &mut self,
        parent_entity: EntityId,
        world_size: Vector2<f32>,
        components: &[GuiComponentRenderInfo],
    ) {
        if self.active_panel != Some(parent_entity) {
            return;
        }
        // `SetUI.world_size` is `screen_size_in_pixels * GUI_PIXEL_TO_WORLD_SIZE`.
        self.panel_size_px = Some(world_size / crate::gui::GUI_PIXEL_TO_WORLD_SIZE);
        self.components = components.to_vec();
    }

    /// The render-target size, for the pointer->canvas letterbox mapping.
    /// Called from the flat render path each frame.
    pub fn set_screen_size(&mut self, screen_size: Vector2<f32>) {
        if screen_size.x > 0.0 && screen_size.y > 0.0 {
            self.screen_size = screen_size;
        }
    }

    /// The active panel's rect on the 640x480 canvas (None until its first
    /// `SetUI` arrives).
    fn panel_rect(&self) -> Option<Rect> {
        self.panel_size_px.map(panel_canvas_rect)
    }

    /// Per-frame pointer processing while in flat presentation. Maps the
    /// normalized pointer to panel-local coordinates and returns the
    /// `GUIHover` message to dispatch to the panel entity; handles the close
    /// gestures (close button, LMB on the bare view, walk-away, entity gone).
    pub fn update(&mut self, world: &World, pointer: Option<Pointer2D>) -> Vec<Message> {
        let pressed = pointer.map(|p| p.pressed).unwrap_or(false);
        let pressed_edge = pressed && !self.last_pointer_pressed;
        self.last_pointer_pressed = pressed;

        let Some(panel) = self.active_panel else {
            self.cursor_canvas = None;
            return Vec::new();
        };

        // The bound object is gone (destroyed / level state changed).
        let alive = world
            .borrow::<EntitiesView>()
            .map(|entities| entities.is_alive(panel))
            .unwrap_or(false);
        if !alive {
            self.close();
            return Vec::new();
        }

        // Walk-away auto-close (the original's per-overlay `distance` check
        // against the bound object).
        let too_far = (|| {
            let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
            let v_pos = world
                .borrow::<View<dark::properties::PropPosition>>()
                .ok()?;
            let pos = v_pos.get(panel).ok()?;
            Some((pos.position - player.pos).magnitude() > PANEL_AUTO_CLOSE_DISTANCE)
        })()
        .unwrap_or(false);
        if too_far {
            self.close();
            return Vec::new();
        }

        let Some(pointer) = pointer else {
            self.cursor_canvas = None;
            return Vec::new();
        };
        let canvas_pos = pointer_to_canvas(
            CANVAS_SIZE,
            pointer.position,
            self.screen_size,
            ScaleMode::PreserveAspect,
        );
        self.cursor_canvas = canvas_pos;

        let Some(canvas_pos) = canvas_pos else {
            // Pointer in the letterbox bars - fully outside the canvas; a
            // click there is a click on the bare view.
            if pressed_edge {
                self.close();
            }
            return Vec::new();
        };

        let Some(rect) = self.panel_rect() else {
            // Panel opened this frame; no SetUI yet - nothing to hit-test.
            return Vec::new();
        };

        // Host-drawn close button (the shared guis have no close component -
        // VR panels never close).
        let close_rect = close_button_canvas_rect(rect);
        self.hover_close = close_rect.contains(canvas_pos);
        if pressed_edge && self.hover_close {
            self.close();
            return Vec::new();
        }

        if rect.contains(canvas_pos) {
            // Forward as GUIHover in panel-local normalized coordinates -
            // the same message the VR hand ray produces, so `GuiScript`'s
            // hit-test/edge-detection runs unchanged. LMB maps to the
            // right-hand trigger; flat has no grab gesture yet.
            let local = point2(
                (canvas_pos.x - rect.x) / rect.w,
                (canvas_pos.y - rect.y) / rect.h,
            );
            vec![Message {
                to: panel,
                payload: MessagePayload::GUIHover {
                    held_entity_id: None,
                    screen_coordinates: local,
                    is_triggered: pointer.pressed,
                    is_grabbing: false,
                    hand: Handedness::Right,
                },
            }]
        } else {
            // LMB on the bare 3D view closes the panels (manual p.7).
            if pressed_edge {
                self.close();
            }
            Vec::new()
        }
    }

    /// Render the active panel + cursor as screen-space overlay objects on
    /// the shared 640x480 canvas (drawn after the flat HUD).
    pub fn render(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let Some(rect) = self.panel_rect() else {
            return Vec::new();
        };
        let mut canvas = UiCanvas::new(CANVAS_SIZE);
        for component in &self.components {
            if is_gui_cursor(component) {
                // GuiScript appends its own panel-local cursor image for the
                // VR quads; the host draws the real screen cursor instead.
                continue;
            }
            let r = component_canvas_rect(component, rect);
            match component {
                // The original MFD art is opaque on screen; the render-info
                // alpha is a VR world-quad translucency, deliberately not
                // applied here.
                GuiComponentRenderInfo::Image { texture, .. } => {
                    canvas.image(r, texture);
                }
                GuiComponentRenderInfo::Text { text, font, .. } => {
                    canvas.text(r, text, font, r.h, HAlign::Left, VAlign::Top);
                }
            }
        }
        canvas.image(
            close_button_canvas_rect(rect),
            if self.hover_close {
                "closeon.pcx"
            } else {
                "closeoff.pcx"
            },
        );
        if let Some(cursor) = self.cursor_canvas {
            canvas.image(
                Rect::new(cursor.x, cursor.y, CURSOR_SIZE.x, CURSOR_SIZE.y),
                "cursor.pcx",
            );
        }
        canvas.render_screen_space(asset_cache, screen_size, ScaleMode::PreserveAspect)
    }

    /// Introspection snapshot of the active panel's elements for `GET /v1/ui`:
    /// canvas + normalized-screen rects and semantic labels, so clients click
    /// widgets by meaning instead of hardcoded pixels.
    pub fn debug_elements(&self) -> Vec<crate::game_scene::DebugUiElement> {
        let Some(rect) = self.panel_rect() else {
            return Vec::new();
        };
        let to_screen = |r: Rect| {
            let s = crate::ui::canvas_rect_to_screen(
                r,
                CANVAS_SIZE,
                self.screen_size,
                ScaleMode::PreserveAspect,
            );
            [s.x, s.y, s.w, s.h]
        };
        let mut out = Vec::new();
        for component in &self.components {
            if is_gui_cursor(component) {
                continue;
            }
            let r = component_canvas_rect(component, rect);
            let (kind, texture, text, label) = match component {
                GuiComponentRenderInfo::Image {
                    texture,
                    interactive,
                    ..
                } => (
                    if *interactive { "button" } else { "image" },
                    Some(texture.clone()),
                    None,
                    if *interactive {
                        semantic_label(texture)
                    } else {
                        None
                    },
                ),
                GuiComponentRenderInfo::Text { text, .. } => {
                    ("text", None, Some(text.clone()), None)
                }
            };
            out.push(crate::game_scene::DebugUiElement {
                kind: kind.to_string(),
                texture,
                text,
                label,
                rect: [r.x, r.y, r.w, r.h],
                screen_rect: to_screen(r),
            });
        }
        // The host-drawn close button is clickable too.
        let close = close_button_canvas_rect(rect);
        out.push(crate::game_scene::DebugUiElement {
            kind: "button".to_string(),
            texture: Some("closeoff.pcx".to_string()),
            text: None,
            label: Some("close".to_string()),
            rect: [close.x, close.y, close.w, close.h],
            screen_rect: to_screen(close),
        });
        out
    }
}

impl Default for FlatUiHost {
    fn default() -> Self {
        Self::new()
    }
}

/// The active panel's rect on the canvas: panel-local pixels anchored at the
/// original left-MFD slot.
fn panel_canvas_rect(panel_size_px: Vector2<f32>) -> Rect {
    Rect::new(
        LEFT_MFD_ANCHOR.x,
        LEFT_MFD_ANCHOR.y,
        panel_size_px.x,
        panel_size_px.y,
    )
}

/// Map one `SetUI` component (normalized panel coordinates) to canvas pixels.
fn component_canvas_rect(info: &GuiComponentRenderInfo, panel: Rect) -> Rect {
    let position = info.position();
    let size = info.size();
    // Text render-info positions carry a negated y (a VR world-quad
    // convention baked into `GuiComponent::to_render_info`); undo it here.
    let y = match info {
        GuiComponentRenderInfo::Text { .. } => -position.y,
        _ => position.y,
    };
    Rect::new(
        panel.x + position.x * panel.w,
        panel.y + y * panel.h,
        size.x * panel.w,
        size.y * panel.h,
    )
}

/// Host-drawn close button: top-right corner of the panel (the original
/// keypad overlay's CloseOff gadget position).
fn close_button_canvas_rect(panel: Rect) -> Rect {
    Rect::new(
        panel.x + panel.w - CLOSE_BUTTON_MARGIN.x,
        panel.y + CLOSE_BUTTON_MARGIN.y,
        CLOSE_BUTTON_SIZE.x,
        CLOSE_BUTTON_SIZE.y,
    )
}

/// Semantic label for a clickable element, derived from its art name. The
/// keypad's digit buttons use the `key<c><0|1>.pcx` convention (`c` = the
/// digit, `n` = clear; the trailing 0/1 is the normal/hover art state), so
/// both art states of a digit label identically.
fn semantic_label(texture: &str) -> Option<String> {
    let t = texture.to_ascii_lowercase();
    let rest = t.strip_prefix("key")?.strip_suffix(".pcx")?;
    let mut chars = rest.chars();
    let (c, state) = (chars.next()?, chars.next()?);
    if chars.next().is_some() || !matches!(state, '0' | '1') {
        return None;
    }
    match c {
        '0'..='9' => Some(c.to_string()),
        'n' => Some("clear".to_string()),
        _ => None,
    }
}

/// `GuiScript` appends a panel-local `cursor.pcx` image for the VR quads;
/// the flat host draws its own screen-space cursor instead.
fn is_gui_cursor(info: &GuiComponentRenderInfo) -> bool {
    matches!(
        info,
        GuiComponentRenderInfo::Image { texture, .. } if texture.eq_ignore_ascii_case("cursor.pcx")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec2;

    #[test]
    fn panel_anchors_at_the_original_left_mfd_slot() {
        // Keypad panel: 188x296 at (2, 124) - the shkmfddm.h left MFD rect.
        let rect = panel_canvas_rect(vec2(188.0, 296.0));
        assert_eq!(rect, Rect::new(2.0, 124.0, 188.0, 296.0));
        // It fits on the 640x480 canvas.
        assert!(rect.y + rect.h <= CANVAS_SIZE.y);
    }

    #[test]
    fn image_components_map_into_the_panel_rect() {
        // The keypad's digit "1" button: panel-local (15, 42, 45x60) is
        // emitted normalized by 188x296; it must land at the anchor + the
        // same panel-local pixels.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let info = GuiComponentRenderInfo::Image {
            position: vec2(15.0 / 188.0, 42.0 / 296.0),
            size: vec2(45.0 / 188.0, 60.0 / 296.0),
            texture: "key10.pcx".to_owned(),
            alpha: 0.5,
            interactive: true,
        };
        let r = component_canvas_rect(&info, panel);
        assert!((r.x - 17.0).abs() < 1e-3);
        assert!((r.y - 166.0).abs() < 1e-3);
        assert!((r.w - 45.0).abs() < 1e-3);
        assert!((r.h - 60.0).abs() < 1e-3);
    }

    #[test]
    fn text_components_undo_the_negated_y_convention() {
        // to_render_info negates text y for the VR quad; canvas mapping must
        // put a panel-local y=20 text at panel top + 20, not above the panel.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let info = GuiComponentRenderInfo::Text {
            position: vec2(10.0 / 188.0, -20.0 / 296.0),
            size: vec2(30.0 / 188.0, 16.0 / 296.0),
            font: "mainfont.fon".to_owned(),
            text: "451".to_owned(),
            alpha: 1.0,
        };
        let r = component_canvas_rect(&info, panel);
        assert!((r.y - (124.0 + 20.0)).abs() < 1e-3);
    }

    #[test]
    fn close_button_hugs_the_top_right_corner() {
        // Matches the original keypad overlay's CloseOff rect (163, 8, 20x21)
        // for the 188-wide panel.
        let panel = panel_canvas_rect(vec2(188.0, 296.0));
        let r = close_button_canvas_rect(panel);
        assert_eq!(r, Rect::new(2.0 + 163.0, 124.0 + 8.0, 20.0, 21.0));
    }

    #[test]
    fn gui_cursor_component_is_recognized() {
        let cursor = GuiComponentRenderInfo::Image {
            position: vec2(0.1, 0.1),
            size: vec2(0.05, 0.05),
            texture: "cursor.pcx".to_owned(),
            alpha: 0.5,
            interactive: false,
        };
        assert!(is_gui_cursor(&cursor));
        let backdrop = GuiComponentRenderInfo::Image {
            position: vec2(0.0, 0.0),
            size: vec2(1.0, 1.0),
            texture: "keypad2.pcx".to_owned(),
            alpha: 0.5,
            interactive: false,
        };
        assert!(!is_gui_cursor(&backdrop));
    }

    #[test]
    fn semantic_labels_identify_keypad_digits() {
        // Both art states of a digit button label as the digit.
        assert_eq!(semantic_label("key40.pcx"), Some("4".to_string()));
        assert_eq!(semantic_label("key41.pcx"), Some("4".to_string()));
        assert_eq!(semantic_label("key00.pcx"), Some("0".to_string()));
        // The clear key (keyn0/keyn1).
        assert_eq!(semantic_label("keyn1.pcx"), Some("clear".to_string()));
        // Non-widget art with a "key" prefix must NOT label.
        assert_eq!(semantic_label("keypad2.pcx"), None);
        assert_eq!(semantic_label("crosshai.pcx"), None);
    }
}
