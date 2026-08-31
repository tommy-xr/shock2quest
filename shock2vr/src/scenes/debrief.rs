//! Character-creation debrief screen.
//!
//! The page the original shows as a training tour ends - "Your stint aboard the
//! UNN Gallo is finished... You've gained +2 Strength." - on its own authored
//! screen: the `DEBRIEF.PCX` backdrop, its `DEBRIEFR.BIN` widget rect for the
//! Continue button, and that button's label from `DEBRIEF.STR`. The body text
//! is the `res/strings/CHARGEN.STR` page the completed tour's reward names.
//!
//! Structurally a sibling of [`crate::scenes::GameOverScene`]: a pointer-driven
//! `GameScene` built on one shared [`UiCanvas`], so flat and VR present the same
//! layout (AGENTS.md section 3). It stands in for the mission scene the way a
//! cutscene does, and emits its `then` effect - the level transition the tour
//! trigger asked for - when Continue is clicked.

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use dark::importers::FONT_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    measure_text_width,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{FrontendMenu, HAlign, Rect, ScaleMode, UiCanvas, VAlign},
};

/// The screen is authored on the original 640x480 `DEBRIEF.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "DEBRIEF.PCX";
/// The screen's widget layout - one LTRB rect (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "DEBRIEFR.BIN";
/// The screen's own string table: a single `Continue` entry.
const LABELS_FILE: &str = "DEBRIEF.STR";
const CONTINUE_KEY: &str = "continue";
const CONTINUE_FALLBACK: &str = "Continue";
/// Same display font the other frontend screens label their buttons with.
const MENU_FONT: &str = "metafont.fon";
/// The small in-game font, for the page body.
const BODY_FONT: &str = "mainfont.fon";
/// Glyph-cell height in canvas pixels for the body, and the row pitch.
/// `mainfont` is ~10 px native; this keeps the page legible at panel distance
/// while a full 27-line-table page still fits the backdrop's text panel.
const BODY_FONT_SIZE: f32 = 16.0;
const BODY_LINE_H: f32 = 19.0;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// The backdrop's central text panel, inset off its border. Read off
/// `DEBRIEF.PCX` (the panel's frame spans x 212..558, y 83..395); the layout
/// file describes only the button, so this rect is not in it.
const TEXT_RECT: Rect = Rect::new(224.0, 96.0, 322.0, 288.0);

/// The one thing this screen does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebriefAction {
    Continue,
}

/// The only index into `DEBRIEFR.BIN`.
const CONTINUE_RECT_INDEX: usize = 0;
/// Decoded `DEBRIEFR.BIN` value, used when the layout file is absent.
const FALLBACK_RECTS: [Rect; 1] = [Rect::new(425.0, 401.0, 210.0, 74.0)];

/// The button at a canvas point, if any. Shared by the click and the rollover
/// sound so the two always agree on where the button is.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<DebriefAction> {
    rects[CONTINUE_RECT_INDEX]
        .contains(point)
        .then_some(DebriefAction::Continue)
}

pub struct DebriefScene {
    world: World,
    scene_name: String,
    /// The page being shown, verbatim (authored line breaks intact).
    text: String,
    /// What Continue does: the transition (behind any departure cutscenes) the
    /// tour trigger asked for.
    then: GlobalEffect,
    menu: FrontendMenu<DebriefAction>,
}

impl DebriefScene {
    pub fn new(text: String, then: GlobalEffect) -> Self {
        Self {
            world: super::ui_scene_world(),
            scene_name: "debrief".to_owned(),
            text,
            then,
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
        }
    }

    /// What a click does: hand on the effect the tour trigger deferred. The
    /// screen never decides where the player goes next.
    fn effects_for(&self, action: Option<DebriefAction>) -> Vec<Effect> {
        match action {
            Some(DebriefAction::Continue) => vec![Effect::GlobalEffect(self.then.clone())],
            None => Vec::new(),
        }
    }

    /// The screen, described once. Both presentations render this canvas, so
    /// they cannot drift apart in layout, wrap, or which button looks
    /// actionable.
    fn build_canvas(
        &self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let lines: Vec<String> = {
            let font = asset_cache.get(&FONT_IMPORTER, BODY_FONT);
            wrap_page(&**font, &self.text, BODY_FONT_SIZE, TEXT_RECT.w)
        };
        for (index, line) in lines.iter().enumerate() {
            let y = TEXT_RECT.y + index as f32 * BODY_LINE_H;
            // A blank line is a paragraph gap: it spaces the block, it draws
            // nothing. Rows past the panel are dropped rather than spilling
            // over the backdrop art.
            if line.is_empty() || y + BODY_LINE_H > TEXT_RECT.y + TEXT_RECT.h {
                continue;
            }
            canvas.text(
                Rect::new(TEXT_RECT.x, y, TEXT_RECT.w, BODY_LINE_H),
                line,
                BODY_FONT,
                BODY_FONT_SIZE,
                HAlign::Left,
                VAlign::Middle,
            );
        }

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, LABELS_FILE);
        let label =
            crate::ui::resolve_menu_label(strings.as_deref(), CONTINUE_KEY, CONTINUE_FALLBACK);
        let rect = rects[CONTINUE_RECT_INDEX];
        let hovered = pointer_canvas.is_some_and(|p| rect.contains(p));
        canvas
            .text_native(rect, &label, MENU_FONT, HAlign::Center, VAlign::Middle)
            .opacity(if hovered { 1.0 } else { 0.6 });

        canvas
    }
}

impl GameScene for DebriefScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        // Only global effects mean anything here: there is no mission to act on.
        let mut effects: Vec<Effect> = command_effects
            .into_iter()
            .filter(|effect| matches!(effect, Effect::GlobalEffect(_)))
            .collect();

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let action = self.menu.update(
            time.elapsed,
            input_context,
            game_options.presentation_mode,
            |point| hit(point, &rects),
            |point| hit(point, &rects),
        );
        effects.extend(self.effects_for(action));
        effects
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let canvas = self.build_canvas(asset_cache, self.menu.pointer_canvas());
        let objects = self
            .menu
            .render_world_space(asset_cache, canvas, options.presentation_mode);
        (objects, vec3(0.0, 0.0, 0.0), identity)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        _view: cgmath::Matrix4<f32>,
        _projection: cgmath::Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        let pointer_canvas = self.menu.screen_pointer_canvas(screen_size);
        let canvas = self.build_canvas(asset_cache, pointer_canvas);
        self.menu
            .render_screen_space(asset_cache, canvas, screen_size, options.presentation_mode)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.menu.pump_sfx(asset_cache, audio_context);
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    fn on_exit(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.menu.stop_sfx(audio_context);
    }

    fn wants_pointer(&self) -> bool {
        true
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }

    /// So `Game` can report the page for `/v1/ui`: a test asserts *which*
    /// debrief came up, not that glyphs appeared.
    fn debrief_text(&self) -> Option<&str> {
        Some(&self.text)
    }
}

/// Lay a page out as rows: authored line breaks are honored (a blank line stays
/// blank, so paragraphs keep their gap) and each authored line is greedily
/// word-wrapped to `max_width` with real glyph widths.
fn wrap_page(font: &dyn engine::Font, text: &str, font_size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let wrapped = wrap_text(font, paragraph, font_size, max_width);
        if wrapped.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(wrapped);
        }
    }
    lines
}

/// Greedy word wrap with real glyph widths. A single word wider than the column
/// gets its own line rather than being dropped.
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

    /// At base height the stub advances 5 per glyph, so a 20-wide column holds
    /// four characters - and a word longer than that still gets a row.
    #[test]
    fn wrap_splits_on_measured_width_and_keeps_wide_words() {
        assert_eq!(
            wrap_text(&FixedFont, "aa bb cc", 10.0, 20.0),
            vec!["aa".to_string(), "bb".to_string(), "cc".to_string()]
        );
        assert_eq!(
            wrap_text(&FixedFont, "aaaaaaa bb", 10.0, 20.0),
            vec!["aaaaaaa".to_string(), "bb".to_string()]
        );
    }

    /// Authored paragraph breaks survive the wrap as blank rows, so the page
    /// keeps the shape `CHARGEN.STR` authors (date / body / grant).
    #[test]
    fn authored_line_breaks_become_their_own_rows() {
        let rows = wrap_page(&FixedFont, "Feb. 12\n\nYou gained", 10.0, 100.0);
        assert_eq!(
            rows,
            vec![
                "Feb. 12".to_string(),
                String::new(),
                "You gained".to_string()
            ]
        );
    }

    /// Continue is the only widget, and it sits where `DEBRIEFR.BIN` puts it.
    #[test]
    fn continue_is_hit_at_the_authored_rect() {
        let rects = FALLBACK_RECTS.to_vec();
        assert_eq!(
            hit(rects[CONTINUE_RECT_INDEX].center(), &rects),
            Some(DebriefAction::Continue)
        );
        assert_eq!(hit(vec2(10.0, 10.0), &rects), None);
    }

    /// Clicking Continue emits exactly the effect the tour trigger deferred -
    /// the screen never decides where the player goes next - and nothing at all
    /// happens until it is clicked.
    #[test]
    fn continuing_dispatches_the_follow_on_effect() {
        let mut scene = DebriefScene::new(
            "page".to_string(),
            GlobalEffect::new_game_transition("station.mis".to_string()),
        );
        let rects = FALLBACK_RECTS.to_vec();
        let center = rects[CONTINUE_RECT_INDEX].center();

        assert!(scene.effects_for(None).is_empty());

        // The menu enters already-pressed, so a held input has to be released
        // before it can click: the first press edge is the second frame.
        scene
            .menu
            .resolve_pointer(Some(center), false, |p| hit(p, &rects), |p| hit(p, &rects));
        let action =
            scene
                .menu
                .resolve_pointer(Some(center), true, |p| hit(p, &rects), |p| hit(p, &rects));
        assert_eq!(action, Some(DebriefAction::Continue));

        let effects = scene.effects_for(action);
        assert!(matches!(
            effects.as_slice(),
            [Effect::GlobalEffect(GlobalEffect::TransitionLevel { .. })]
        ));
    }

    #[test]
    fn world_supports_transition_save_data() {
        let scene = DebriefScene::new("page".to_string(), GlobalEffect::Quit);
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
