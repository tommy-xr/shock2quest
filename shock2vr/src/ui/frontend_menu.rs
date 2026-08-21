//! Shared lifecycle, input and presentation shell for frontend menus.
//!
//! A frontend screen still owns its screen-specific canvas contents and action
//! handling. This type owns the parts that must not drift between screens:
//! authored layout/string resolution, flat/VR pointer reduction, rising-edge
//! clicks, panel placement, pointer visuals, frontend sound, and the two canvas
//! presentations.

use std::{collections::HashMap, time::Duration};

use cgmath::Vector2;
use dark::{
    importers::{STRINGS_IMPORTER, UI_LAYOUT_IMPORTER},
    map::MapRect,
};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;

use crate::{
    PresentationMode,
    input_context::{InputContext, Pointer2D},
};

use super::{
    FrontendCanvasPresenter, FrontendPanelAnchor, FrontendPointerPass, PointerVisuals, Rect,
    ScaleMode, UiCanvas, WorldPanel, pointer_to_canvas, vr_frontend_pointer_pass,
};

use super::frontend_sfx::FrontendSfx;

/// One authored menu entry. Its rect is the same-index entry in the screen's
/// `*R.BIN` layout and its label comes from the screen's `*.STR` table.
pub struct FrontendMenuItem<A> {
    pub string_key: &'static str,
    pub fallback_label: &'static str,
    /// `None` draws an inert entry and gives it no hit region.
    pub action: Option<A>,
    /// An honest replacement for an authored label (for example the temporary
    /// Developer entry hosted in the original Options slot).
    pub label_override: Option<&'static str>,
}

/// Resolve authored LTRB rectangles, falling back per entry when the layout is
/// absent or shorter than expected.
pub fn resolve_menu_rects(layout: Option<&[MapRect]>, fallback: &[Rect]) -> Vec<Rect> {
    fallback
        .iter()
        .enumerate()
        .map(|(index, fallback)| {
            layout
                .and_then(|rects| rects.get(index))
                .map(|rect| {
                    Rect::new(
                        rect.ul_x as f32,
                        rect.ul_y as f32,
                        rect.width() as f32,
                        rect.height() as f32,
                    )
                })
                .unwrap_or(*fallback)
        })
        .collect()
}

/// Resolve a string-table value without allowing a missing/empty localized
/// entry to blank a widget.
pub fn resolve_menu_label(
    strings: Option<&HashMap<String, String>>,
    key: &str,
    fallback: &str,
) -> String {
    strings
        // The strings importer lowercases its keys.
        .and_then(|table| table.get(key))
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// Resolve a label parallel to every item in an authored menu.
pub fn resolve_menu_labels<A>(
    strings: Option<&HashMap<String, String>>,
    items: &[FrontendMenuItem<A>],
) -> Vec<String> {
    items
        .iter()
        .map(|item| {
            item.label_override.map(str::to_owned).unwrap_or_else(|| {
                resolve_menu_label(strings, item.string_key, item.fallback_label)
            })
        })
        .collect()
}

/// Hit-test enabled item actions against their same-index authored rectangles.
pub fn hit_menu_item<A: Copy>(
    point: Vector2<f32>,
    items: &[FrontendMenuItem<A>],
    rects: &[Rect],
    enabled: impl Fn(A) -> bool,
) -> Option<A> {
    items.iter().zip(rects).find_map(|(item, rect)| {
        item.action
            .filter(|action| enabled(*action) && rect.contains(point))
    })
}

/// Rising-edge click resolution after either presentation has reduced its
/// pointer to canvas space.
pub fn resolve_click_at<A>(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    hit: impl FnOnce(Vector2<f32>) -> Option<A>,
) -> (Option<A>, bool) {
    if !pressed || last_pressed {
        return (None, pressed);
    }
    (point.and_then(hit), pressed)
}

/// Flat pointer state in the same `(canvas point, pressed)` shape emitted by
/// the VR pointer pass. Pointer loss is deliberately not a release: the cursor
/// can disappear for a frame while capture changes, and clearing the latch
/// there would turn a held button into a new press when the pointer returns.
pub fn flat_pointer_state(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    canvas_size: Vector2<f32>,
    scale_mode: ScaleMode,
) -> (Option<Vector2<f32>>, bool) {
    match pointer {
        Some(pointer) => (
            pointer_to_canvas(canvas_size, pointer.position, screen_size, scale_mode),
            pointer.pressed,
        ),
        None => (None, last_pressed),
    }
}

/// Pure flat click composition, primarily useful to keep screen-level behavior
/// tests independent from an asset cache or runtime.
#[cfg(test)]
pub fn resolve_flat_click<A>(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    canvas_size: Vector2<f32>,
    scale_mode: ScaleMode,
    hit: impl FnOnce(Vector2<f32>) -> Option<A>,
) -> (Option<A>, bool, Option<Vector2<f32>>) {
    let (point, pressed) =
        flat_pointer_state(pointer, last_pressed, screen_size, canvas_size, scale_mode);
    let (action, pressed) = resolve_click_at(point, pressed, last_pressed, hit);
    (action, pressed, point)
}

/// Shared state and presentation boundary for one frontend menu.
pub struct FrontendMenu<A> {
    canvas_size: Vector2<f32>,
    scale_mode: ScaleMode,
    pointer: Option<Pointer2D>,
    pointer_canvas: Option<Vector2<f32>>,
    vr_pointer: FrontendPointerPass,
    vr_pointer_visuals: PointerVisuals,
    panel_anchor: FrontendPanelAnchor,
    last_pressed: bool,
    last_screen_size: Vector2<f32>,
    sfx: FrontendSfx<A>,
}

impl<A: Copy + PartialEq> FrontendMenu<A> {
    pub fn new(canvas_size: Vector2<f32>, scale_mode: ScaleMode) -> Self {
        Self {
            canvas_size,
            scale_mode,
            pointer: None,
            pointer_canvas: None,
            vr_pointer: FrontendPointerPass::default(),
            vr_pointer_visuals: PointerVisuals::new(),
            panel_anchor: FrontendPanelAnchor::new(),
            // Enter every frontend surface already pressed. A held gameplay or
            // previous-screen input must be released before it can click here.
            last_pressed: true,
            last_screen_size: canvas_size,
            sfx: FrontendSfx::new(),
        }
    }

    /// Resolve a screen's layout through the shell that consumes it.
    pub fn rects(
        &self,
        asset_cache: &mut AssetCache,
        layout_file: &str,
        fallback: &[Rect],
    ) -> Vec<Rect> {
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, layout_file);
        resolve_menu_rects(layout.as_deref().map(|rects| rects.as_slice()), fallback)
    }

    /// Resolve a screen's item labels through the shell that renders them.
    pub fn labels<B>(
        &self,
        asset_cache: &mut AssetCache,
        labels_file: &str,
        items: &[FrontendMenuItem<B>],
    ) -> Vec<String> {
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, labels_file);
        resolve_menu_labels(strings.as_deref(), items)
    }

    /// Advance panel placement and resolve one pointer frame. Click, hover,
    /// highlight and pointer visuals all consume the returned shared point.
    pub fn update(
        &mut self,
        elapsed: Duration,
        input_context: &InputContext,
        presentation_mode: PresentationMode,
        click_hit: impl FnOnce(Vector2<f32>) -> Option<A>,
        hover_hit: impl FnOnce(Vector2<f32>) -> Option<A>,
    ) -> Option<A> {
        let panel = self.panel_anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            elapsed,
        );

        let (point, pressed) = if presentation_mode == PresentationMode::Vr {
            self.vr_pointer = vr_frontend_pointer_pass(input_context, self.canvas_size, &panel);
            self.pointer = None;
            (self.vr_pointer.point(), self.vr_pointer.pressed)
        } else {
            self.pointer = input_context.pointer;
            self.vr_pointer = FrontendPointerPass::default();
            flat_pointer_state(
                input_context.pointer,
                self.last_pressed,
                self.last_screen_size,
                self.canvas_size,
                self.scale_mode,
            )
        };
        self.pointer_canvas = point;

        self.resolve_pointer(point, pressed, click_hit, hover_hit)
    }

    /// Consume an already-resolved point/press pair. Reusable overlays call
    /// this in focused page-turn tests so those tests exercise the same latch
    /// and sound path as the runtime update.
    pub fn resolve_pointer(
        &mut self,
        point: Option<Vector2<f32>>,
        pressed: bool,
        click_hit: impl FnOnce(Vector2<f32>) -> Option<A>,
        hover_hit: impl FnOnce(Vector2<f32>) -> Option<A>,
    ) -> Option<A> {
        self.pointer_canvas = point;
        let (action, pressed) = resolve_click_at(point, pressed, self.last_pressed, click_hit);
        self.last_pressed = pressed;
        self.sfx.hover(point.and_then(hover_hit));
        if action.is_some() {
            self.sfx.click();
        }
        action
    }

    /// Clear transient pointer state when an overlay closes.
    pub fn clear_pointer(&mut self) {
        self.pointer = None;
        self.pointer_canvas = None;
        self.vr_pointer = FrontendPointerPass::default();
    }

    /// Reset reusable overlay state on entry. Starting pressed prevents the
    /// input that opened it from activating a widget in the same hold.
    pub fn reset_on_entry(&mut self) {
        self.clear_pointer();
        self.panel_anchor = FrontendPanelAnchor::new();
        self.last_pressed = true;
        self.sfx = FrontendSfx::new();
    }

    pub fn pointer_canvas(&self) -> Option<Vector2<f32>> {
        self.pointer_canvas
    }

    /// Update the current target size and map the flat pointer exactly as the
    /// screen-space presenter maps the canvas.
    pub fn screen_pointer_canvas(&mut self, screen_size: Vector2<f32>) -> Option<Vector2<f32>> {
        self.last_screen_size = screen_size;
        self.pointer.and_then(|pointer| {
            pointer_to_canvas(
                self.canvas_size,
                pointer.position,
                screen_size,
                self.scale_mode,
            )
        })
    }

    pub fn panel(&self) -> WorldPanel {
        self.panel_anchor.panel()
    }

    /// Present an already-resolved canvas on the shared honest-basis panel and
    /// append pointer visuals from the same arbitration pass used by update.
    pub fn render_world_space(
        &mut self,
        asset_cache: &mut AssetCache,
        canvas: UiCanvas,
        presentation_mode: PresentationMode,
    ) -> Vec<SceneObject> {
        let panel = self.panel();
        FrontendCanvasPresenter::new(presentation_mode, self.scale_mode).render_world_space_with(
            asset_cache,
            &canvas,
            &panel,
            self.pointer_canvas,
            |asset_cache, objects| {
                let panel_layers = objects.len();
                objects.extend(self.vr_pointer_visuals.render(
                    asset_cache,
                    &self.vr_pointer,
                    self.canvas_size,
                    &panel,
                    panel_layers,
                ));
            },
        )
    }

    pub fn render_screen_space(
        &self,
        asset_cache: &mut AssetCache,
        canvas: UiCanvas,
        screen_size: Vector2<f32>,
        presentation_mode: PresentationMode,
    ) -> Vec<SceneObject> {
        FrontendCanvasPresenter::new(presentation_mode, self.scale_mode).render_screen_space(
            asset_cache,
            &canvas,
            screen_size,
        )
    }

    pub fn pump_sfx(
        &mut self,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        self.sfx.pump(asset_cache, audio_context);
    }

    pub fn stop_sfx(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.sfx.stop(audio_context);
    }

    #[cfg(test)]
    pub fn set_last_pressed(&mut self, pressed: bool) {
        self.last_pressed = pressed;
    }

    #[cfg(test)]
    pub fn last_pressed(&self) -> bool {
        self.last_pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec2;

    use crate::ui::{FrontendCanvasPresenter, VR_COMPONENT_Z_STEP};

    #[test]
    fn frontend_presenter_selects_one_target_and_keeps_geometry_policy() {
        let flat = FrontendCanvasPresenter::new(PresentationMode::Flat, ScaleMode::PreserveAspect);
        assert!(!flat.renders_world_space());
        assert!(flat.renders_screen_space());
        assert_eq!(flat.scale_mode(), ScaleMode::PreserveAspect);
        assert_eq!(flat.component_z_step(), VR_COMPONENT_Z_STEP);

        let vr = FrontendCanvasPresenter::new(PresentationMode::Vr, ScaleMode::PreserveAspect);
        assert!(vr.renders_world_space());
        assert!(!vr.renders_screen_space());
        assert_eq!(vr.scale_mode(), ScaleMode::PreserveAspect);
        assert_eq!(vr.component_z_step(), VR_COMPONENT_Z_STEP);
    }

    #[test]
    fn original_frontend_scenes_do_not_own_presentation_mode_branches() {
        let consumers = [
            ("main menu", include_str!("../scenes/main_menu.rs")),
            ("load game", include_str!("../scenes/load_game.rs")),
            ("game over", include_str!("../scenes/game_over.rs")),
            ("loading", include_str!("../scenes/loading.rs")),
            ("no assets", include_str!("../scenes/no_assets.rs")),
        ];

        for (name, source) in consumers {
            assert!(
                !source.contains("options.presentation_mode != PresentationMode::Vr")
                    && !source.contains("options.presentation_mode == PresentationMode::Vr"),
                "{name} must delegate flat/VR presentation selection to FrontendCanvasPresenter"
            );
        }
    }

    #[test]
    fn pointer_loss_does_not_rearm_a_held_press() {
        let canvas = vec2(640.0, 480.0);
        let screen = vec2(800.0, 600.0);
        let (action, last_pressed, point) = resolve_flat_click(
            None,
            true,
            screen,
            canvas,
            ScaleMode::PreserveAspect,
            |_| Some(()),
        );
        assert_eq!(action, None);
        assert_eq!(point, None);
        assert!(
            last_pressed,
            "a missing pointer is not evidence of a release"
        );
    }
}
