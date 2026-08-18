//! Transient narration text: the on-screen lines for a playing voice-over.
//!
//! One overlay owned by [`crate::Game`], fed by
//! [`GlobalEffect::ShowSubtitle`](crate::scripts::GlobalEffect) and ticked on
//! the sim clock (it freezes with the scene under the pause menu). The text
//! lives on a single shared [`UiCanvas`] - the placement decision (wrapped
//! lines, bottom-center of the 640x480 canvas) is made exactly once, in canvas
//! pixels, per AGENTS.md section 3:
//!
//! - **Flat** renders that canvas in screen space (aspect-preserving, like
//!   every other flat screen), so the lines sit at the bottom of the window -
//!   where the remaster draws its subtitles.
//! - **VR** renders the *same canvas* on a world panel hung from the shared
//!   [`FrontendPanelAnchor`]: placed from the head pose when a narration
//!   starts, yaw-only and gravity-aligned, world-locked while it plays, lazily
//!   recentered only after a sustained large head deviation (vr-ui-design
//!   rule 3 - never gaze-glued). The bottom-of-canvas placement lands the text
//!   slightly below the view center at the panel's 2 m focal distance.
//!
//! Both presentations therefore show identical content at the same canvas
//! position; neither makes any placement decision of its own.

use cgmath::{Matrix4, Vector2, vec2};
use dark::importers::FONT_IMPORTER;
use engine::{assets::asset_cache::AssetCache, measure_text_width, scene::SceneObject};
use std::time::Duration;

use crate::{
    GameOptions, PresentationMode,
    input_context::InputContext,
    subtitles::SubtitleCue,
    ui::{FrontendPanelAnchor, HAlign, Rect, ScaleMode, UiCanvas, VAlign, VR_COMPONENT_Z_STEP},
    util::{render_source, tag_render_source},
};

/// Authored on the same 640x480 canvas as the original UI art.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// Text column width; the rest is side margin.
const TEXT_W: f32 = 520.0;
/// Where the *bottom* line of text sits. On the VR panel (1.5 m tall at 2 m)
/// this puts the text ~15 degrees below the view center - subtitle height.
const TEXT_BOTTOM: f32 = 440.0;
/// Glyph-cell height in canvas pixels. `mainfont` is ~10 px native; doubling
/// it keeps the line count low and the text legible at the panel distance.
const FONT_SIZE: f32 = 20.0;
const LINE_H: f32 = 24.0;
const FONT: &str = "mainfont.fon";
/// Flat letterboxes like every other flat screen.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// The narration currently on screen.
struct ActiveSubtitle {
    /// Sample the cues belong to, for the repeat-while-playing guard.
    sample: String,
    cues: Vec<SubtitleCue>,
    /// Sim time since the narration started.
    clock: Duration,
    /// When the last cue is done and the overlay goes idle.
    total: Duration,
}

/// See the module docs.
pub struct SubtitleOverlay {
    active: Option<ActiveSubtitle>,
    /// VR panel placement, reset when a new narration starts so the toast is
    /// hung from the head pose that heard it (vr-ui-design rule 3).
    anchor: FrontendPanelAnchor,
}

impl Default for SubtitleOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl SubtitleOverlay {
    pub fn new() -> Self {
        Self {
            active: None,
            anchor: FrontendPanelAnchor::new(),
        }
    }

    /// Start showing a sample's cues.
    ///
    /// Re-posting the sample already on screen is ignored (the data marks
    /// these narrations `singleton`, and level wiring can fire one trap from
    /// several switch links in the same instant). A *different* sample
    /// replaces the current one - last narration wins, like the remaster.
    pub fn post(&mut self, sample: &str, cues: Vec<SubtitleCue>) {
        if cues.is_empty() {
            return;
        }
        if let Some(active) = &self.active {
            if active.sample.eq_ignore_ascii_case(sample) {
                return;
            }
        }
        let total = cues
            .iter()
            .map(SubtitleCue::end)
            .max()
            .unwrap_or(Duration::ZERO);
        self.active = Some(ActiveSubtitle {
            sample: sample.to_owned(),
            cues,
            clock: Duration::ZERO,
            total,
        });
        // Place the panel fresh from the head pose the narration starts at.
        self.anchor = FrontendPanelAnchor::new();
    }

    /// Advance the clock and the VR panel anchor. Call on the *scene* clock:
    /// while the pause menu suspends the scene this must not run, so the text
    /// freezes with the world instead of expiring under the menu.
    pub fn update(&mut self, elapsed: Duration, input_context: &InputContext) {
        let Some(active) = &mut self.active else {
            return;
        };
        active.clock += elapsed;
        if active.clock >= active.total {
            self.active = None;
            return;
        }
        self.anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            elapsed,
        );
    }

    pub fn is_visible(&self) -> bool {
        self.active.is_some()
    }

    /// The cue lines on screen right now (a multisub can leave gaps between
    /// cues, where nothing shows - matching the authored timing).
    fn visible_lines(&self) -> Vec<&str> {
        let Some(active) = &self.active else {
            return Vec::new();
        };
        active
            .cues
            .iter()
            .filter(|cue| active.clock >= cue.start && active.clock < cue.end())
            .map(|cue| cue.text.as_str())
            .collect()
    }

    /// The one shared canvas: visible cue text, greedily wrapped with real
    /// glyph measurement, as bottom-anchored centered rows.
    fn build_canvas(&self, asset_cache: &mut AssetCache) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        let lines: Vec<String> = {
            let font = asset_cache.get(&FONT_IMPORTER, FONT);
            self.visible_lines()
                .iter()
                .flat_map(|text| wrap_text(&**font, text, FONT_SIZE, TEXT_W))
                .collect()
        };
        let top = TEXT_BOTTOM - lines.len() as f32 * LINE_H;
        for (index, line) in lines.iter().enumerate() {
            canvas.text(
                Rect::new(
                    (CANVAS_W - TEXT_W) / 2.0,
                    top + index as f32 * LINE_H,
                    TEXT_W,
                    LINE_H,
                ),
                line,
                FONT,
                FONT_SIZE,
                HAlign::Center,
                VAlign::Middle,
            );
        }
        canvas
    }

    /// World-space presentation (VR): the canvas on the anchored panel.
    /// Empty when idle or flat. `pawn_to_world` rebases the tracked play
    /// space into world coordinates, exactly like the pause menu.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
        pawn_to_world: Matrix4<f32>,
    ) -> Vec<SceneObject> {
        if !self.is_visible() || options.presentation_mode != PresentationMode::Vr {
            return Vec::new();
        }
        let panel = self.anchor.panel();
        let canvas = self.build_canvas(asset_cache);
        let mut objects = canvas.render_world_space(
            asset_cache,
            panel.transform(),
            None,
            None,
            VR_COMPONENT_Z_STEP,
        );
        for object in &mut objects {
            object.set_transform(pawn_to_world * object.get_transform());
        }
        tag_render_source(&mut objects, render_source::SUBTITLE);
        objects
    }

    /// Screen-space presentation (flat). Empty when idle or in VR (a
    /// screen-space copy would paste over both eyes).
    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        if !self.is_visible() || options.presentation_mode == PresentationMode::Vr {
            return Vec::new();
        }
        let canvas = self.build_canvas(asset_cache);
        let mut objects = canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE);
        tag_render_source(&mut objects, render_source::SUBTITLE);
        objects
    }
}

/// Greedy word wrap with real glyph widths. A single word wider than the
/// column gets its own line rather than being dropped.
fn wrap_text(font: &dyn engine::Font, text: &str, font_size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if measure_text_width(font, &candidate, font_size) <= max_width || current.is_empty() {
            current = candidate;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_owned();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_context::InputContext;

    fn cue(start_ms: u64, length_ms: u64, text: &str) -> SubtitleCue {
        SubtitleCue {
            start: Duration::from_millis(start_ms),
            length: Some(Duration::from_millis(length_ms)),
            text: text.to_owned(),
        }
    }

    fn tick(overlay: &mut SubtitleOverlay, ms: u64) {
        overlay.update(Duration::from_millis(ms), &InputContext::default());
    }

    #[test]
    fn cues_appear_and_expire_on_their_timings() {
        let mut overlay = SubtitleOverlay::new();
        overlay.post(
            "trg0001",
            vec![cue(0, 1000, "first"), cue(1500, 1000, "second")],
        );
        assert_eq!(overlay.visible_lines(), vec!["first"]);
        tick(&mut overlay, 1200);
        assert!(
            overlay.visible_lines().is_empty(),
            "the gap between cues shows nothing"
        );
        tick(&mut overlay, 500);
        assert_eq!(overlay.visible_lines(), vec!["second"]);
        tick(&mut overlay, 1000);
        assert!(
            !overlay.is_visible(),
            "overlay goes idle after the last cue"
        );
    }

    #[test]
    fn reposting_the_playing_sample_does_not_restart_it() {
        let mut overlay = SubtitleOverlay::new();
        overlay.post("trg0001", vec![cue(0, 1000, "line")]);
        tick(&mut overlay, 600);
        overlay.post("TRG0001", vec![cue(0, 1000, "line")]);
        tick(&mut overlay, 600);
        assert!(
            !overlay.is_visible(),
            "a duplicate post must not rewind the clock"
        );
    }

    #[test]
    fn a_new_sample_replaces_the_current_one() {
        let mut overlay = SubtitleOverlay::new();
        overlay.post("trg0001", vec![cue(0, 5000, "old")]);
        overlay.post("trg0002", vec![cue(0, 1000, "new")]);
        assert_eq!(overlay.visible_lines(), vec!["new"]);
    }

    #[test]
    fn empty_cues_are_ignored() {
        let mut overlay = SubtitleOverlay::new();
        overlay.post("trg0001", Vec::new());
        assert!(!overlay.is_visible());
    }

    #[test]
    fn wrap_splits_on_measured_width_and_keeps_wide_words() {
        /// Fixed-metrics stub: every glyph advances 5 at base height 10.
        struct FixedFont;
        impl engine::Font for FixedFont {
            fn get_texture(&self) -> std::rc::Rc<dyn engine::texture::TextureTrait> {
                unreachable!("measurement does not touch the texture")
            }
            fn get_character_info(&self, _c: char) -> Option<engine::FontCharacterInfo> {
                Some(engine::FontCharacterInfo {
                    min_uv_x: 0.0,
                    min_uv_y: 0.0,
                    max_uv_x: 1.0,
                    max_uv_y: 1.0,
                    advance: 5.0,
                })
            }
            fn base_height(&self) -> f32 {
                10.0
            }
            fn get_half_pixel(&self) -> f32 {
                0.0
            }
        }
        // 5px per char at size 10; max 30px = 6 chars per line.
        let lines = wrap_text(&FixedFont, "aa bb cc unbreakable", 10.0, 30.0);
        assert_eq!(lines, vec!["aa bb", "cc", "unbreakable"]);
    }
}
