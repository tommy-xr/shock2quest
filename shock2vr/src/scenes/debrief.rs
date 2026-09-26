//! Character-creation debrief screen.
//!
//! The page the original shows as a training tour ends - "Your stint aboard the
//! UNN Gallo is finished... You've gained +2 Strength." - on its own authored
//! screen: `DEBRIEF.PCX`, its `DEBRIEFR.BIN` Continue rect and `DEBRIEF.STR`
//! label, plus the service banner and tour illustration. Retail's
//! `shkdebrf.cpp` supplies the text/art placements; `CHARGEN.STR` supplies
//! the page, mission-heading format and illustration key, and `USEMSG.STR`
//! supplies the posting. Both text faces retain their authored palette.
//!
//! Structurally a sibling of [`crate::scenes::GameOverScene`]: a pointer-driven
//! `GameScene` built on one shared [`UiCanvas`], so flat and VR present the same
//! layout (AGENTS.md section 3). It stands in for the mission scene the way a
//! cutscene does, and emits its `then` effect - the level transition the tour
//! trigger asked for - when Continue is clicked.

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use dark::importers::STRINGS_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    measure_text_width,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};
use tracing::warn;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        FrontendMenu, FrontendMenuItem, HAlign, MFD_FONT, Rect, ScaleMode, TITLE_FONT, UiCanvas,
        VAlign, hit_menu_item, resolve_font,
    },
};

/// The screen is authored on the original 640x480 `DEBRIEF.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "DEBRIEF.PCX";
/// The screen's widget layout - one LTRB rect (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "DEBRIEFR.BIN";
/// The screen's own string table: a single `Continue` entry.
const LABELS_FILE: &str = "DEBRIEF.STR";
/// Same display font the other frontend screens label their buttons with.
const MENU_FONT: &str = "metafont.fon";
/// Retail's gShockFontBlue (BLUEAA.FON) and gShockFont (MAINAA.FON).
/// These shared aliases preserve the fontpal.pcx colors in both presentations.
const BODY_FONT: &str = TITLE_FONT;
const MISSION_FONT: &str = MFD_FONT;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Retail src/shock/shkdebrf.cpp hardcodes these placements on the 640x480
/// canvas; DEBRIEFR.BIN only supplies Continue. Text starts at each rect's
/// top-left, with native glyph-height row pitch (BLUEAA 20px, MAINAA 12px).
const TEXT_RECT: Rect = Rect::new(216.0, 92.0, 336.0, 294.0);
const MISSION_RECT: Rect = Rect::new(6.0, 6.0, 200.0, 75.0);
/// Retail blits CGTITLE at (212,4) inside a 353x80 region, without stretching:
/// the three shipped images are 347x79. Likewise D001..D006 are 204x156.
const LOGO_RECT: Rect = Rect::new(212.0, 4.0, 347.0, 79.0);
const ART_RECT: Rect = Rect::new(4.0, 320.0, 204.0, 156.0);
const SERVICE_TEXTURES: [&str; 3] = ["CGTITLE1.PCX", "CGTITLE2.PCX", "CGTITLE3.PCX"];

/// The one thing this screen does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebriefAction {
    Continue,
}

/// The screen's one widget, parallel to `DEBRIEFR.BIN`'s one rect.
const MENU_ITEMS: &[FrontendMenuItem<DebriefAction>] = &[FrontendMenuItem {
    string_key: "continue",
    fallback_label: "Continue",
    action: Some(DebriefAction::Continue),
    label_override: None,
}];
const CONTINUE_RECT_INDEX: usize = 0;
/// Decoded `DEBRIEFR.BIN` value, used when the layout file is absent.
const FALLBACK_RECTS: [Rect; 1] = [Rect::new(425.0, 401.0, 210.0, 74.0)];

/// The button at a canvas point, if any. Shared by the click and the rollover
/// sound so the two always agree on where the button is.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<DebriefAction> {
    hit_menu_item(point, MENU_ITEMS, rects, |_| true)
}

pub struct DebriefScene {
    world: World,
    scene_name: String,
    /// The page being shown, verbatim (authored line breaks intact).
    text: String,
    /// Mission1..Mission27 also selects the posting, service banner and art.
    mission: Option<u8>,
    /// What Continue does: the transition (behind any departure cutscenes) the
    /// tour trigger asked for.
    then: GlobalEffect,
    menu: FrontendMenu<DebriefAction>,
    /// Latches the "page too tall for the panel" warning, which is otherwise
    /// decided again on every rendered frame.
    truncation_warned: bool,
}

impl DebriefScene {
    pub fn new(text_key: &str, text: String, then: GlobalEffect) -> Self {
        Self {
            world: super::ui_scene_world(),
            scene_name: "debrief".to_owned(),
            text,
            mission: text_key
                .to_ascii_lowercase()
                .strip_prefix("mission")
                .and_then(|number| number.parse::<u8>().ok())
                .filter(|number| (1..=27).contains(number)),
            then,
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
            truncation_warned: false,
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
        &mut self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let mut heading = String::new();
        if let Some(mission) = self.mission {
            canvas.image(LOGO_RECT, SERVICE_TEXTURES[usize::from((mission - 1) / 9)]);
            if let Some(strings) = asset_cache.get_opt(&STRINGS_IMPORTER, "chargen.str") {
                if let Some(picture) =
                    crate::player_stats::debrief_text(&strings, &format!("missionpic{mission}"))
                {
                    canvas.image(ART_RECT, &format!("{picture}.pcx"));
                }
                if let Some(postings) = asset_cache.get_opt(&STRINGS_IMPORTER, "usemsg.str") {
                    if let (Some(format), Some(posting)) = (
                        strings.get("missiontitle"),
                        crate::player_stats::debrief_text(&postings, &format!("post{mission}")),
                    ) {
                        // Retail uses the complete PostN, including its gain line.
                        heading = format.replace("%s", &posting);
                    }
                }
            }
        }

        for (text, font_name, rect) in [
            (self.text.as_str(), BODY_FONT, TEXT_RECT),
            (heading.as_str(), MISSION_FONT, MISSION_RECT),
        ] {
            let font = resolve_font(asset_cache, font_name);
            let line_height = font.base_height();
            let lines = wrap_page(&**font, text, line_height, rect.w);
            for (index, line) in lines.iter().enumerate() {
                let y = rect.y + index as f32 * line_height;
                // All 27 retail pages fit at native size. Warn for oversized
                // replacement strings instead of painting over the frame.
                if y + line_height > rect.y + rect.h {
                    if !self.truncation_warned {
                        self.truncation_warned = true;
                        warn!("Debrief text exceeds its retail rect - truncated");
                    }
                    break;
                }
                if !line.is_empty() {
                    canvas.text_native_fit(
                        Rect::new(rect.x, y, rect.w, line_height),
                        line,
                        font_name,
                        HAlign::Left,
                        VAlign::Top,
                    );
                }
            }
        }

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let labels = self.menu.labels(asset_cache, LABELS_FILE, MENU_ITEMS);
        let label = &labels[CONTINUE_RECT_INDEX];
        let rect = rects[CONTINUE_RECT_INDEX];
        let hovered = pointer_canvas.is_some_and(|p| rect.contains(p));
        canvas
            .text_native(rect, label, MENU_FONT, HAlign::Center, VAlign::Middle)
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
        let pointer_canvas = self.menu.pointer_canvas();
        let canvas = self.build_canvas(asset_cache, pointer_canvas);
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

/// Retail gr_font_string_wrap: retain authored spaces within a row, replace
/// the last fitting space with a line break, and swallow one following space.
/// The general engine wrapper normalizes whitespace and splits long words;
/// that changes the line breaks of CHARGEN.STR's double-spaced sentences.
fn wrap_text(font: &dyn engine::Font, text: &str, font_size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let mut last_space = None;
        let mut split = None;
        for end in remaining
            .char_indices()
            .filter_map(|(i, c)| (c == ' ').then_some(i))
            .chain(std::iter::once(remaining.len()))
        {
            if measure_text_width(font, &remaining[..end], font_size) > max_width {
                split = last_space.or_else(|| (end < remaining.len()).then_some(end));
                break;
            }
            last_space = Some(end);
        }
        match split {
            Some(index) => {
                lines.push(remaining[..index].to_owned());
                remaining = &remaining[index + 1..];
                remaining = remaining.strip_prefix(' ').unwrap_or(remaining);
            }
            None => {
                lines.push(remaining.to_owned());
                break;
            }
        }
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

    #[test]
    fn retail_wrap_preserves_sentence_spacing_and_consumes_wrap_spaces() {
        // CHARGEN uses double spaces between sentences. Collapsing them moves
        // the next word onto the preceding row, unlike gr_font_string_wrap.
        assert_eq!(
            wrap_page(&FixedFont, "aa  bb cc", 10.0, 40.0),
            ["aa  bb", "cc"]
        );
        assert_eq!(wrap_page(&FixedFont, "aa  bb", 10.0, 20.0), ["aa ", "bb"]);
        assert_eq!(
            wrap_page(&FixedFont, "aaaa  bb", 10.0, 20.0),
            ["aaaa", "bb"]
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
            "Mission1",
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
        let scene = DebriefScene::new("Mission1", "page".to_string(), GlobalEffect::Quit);
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
