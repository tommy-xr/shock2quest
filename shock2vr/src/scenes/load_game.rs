//! Load-game screen ("the Tri-Optimum archive database").
//!
//! The original `GAMELOD.PCX` backdrop with its `GAMELODR.BIN` widget rects: a
//! header line, a scrolling list of saves, and "Load" / "Done" buttons. Labels
//! come from `GAMELOD.STR`, so nothing about the screen's text or geometry is
//! hardcoded (see the sibling [`crate::scenes::MainMenuScene`]).
//!
//! Reached from the main menu's "Load Game" entry; "Done" returns there.
//! [`crate::scenes::GameOverScene`] is a degenerate cousin of this screen -
//! same backdrop and layout, but offering only the single most recent save.

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use dark::{
    importers::{STRINGS_IMPORTER, UI_LAYOUT_IMPORTER},
    map::MapRect,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};
use std::collections::HashMap;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::{InputContext, Pointer2D},
    mission::GlobalContext,
    save_load::{SaveFile, all_saves},
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
};

/// The screen is authored on the original 640x480 `GAMELOD.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
/// The original load screen's widget layout - LTRB rects (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "GAMELODR.BIN";
/// The original label strings for this screen.
const LABELS_FILE: &str = "GAMELOD.STR";
/// Same display font the rest of the frontend uses (`res/intrface/METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Indices into the `GAMELODR.BIN` rect list, in the screen's authored order.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const LOAD_RECT_INDEX: usize = 2;
const DONE_RECT_INDEX: usize = 3;

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent.
const FALLBACK_RECTS: [Rect; 4] = [
    Rect::new(261.0, 31.0, 202.0, 20.0),
    Rect::new(261.0, 54.0, 202.0, 290.0),
    Rect::new(527.0, 161.0, 96.0, 62.0),
    Rect::new(527.0, 405.0, 95.0, 62.0),
];

/// Height of one save row inside the list rect, in canvas pixels. Matches the
/// 20px cell of `METAFONT.FON`, which is what the list was authored around -
/// the vanilla 290px-tall list therefore holds 14 rows.
const ROW_HEIGHT: f32 = 20.0;

/// Opacity for a row or button that cannot be acted on.
const DISABLED_OPACITY: f32 = 0.3;
/// Opacity for an actionable, unhighlighted element.
const IDLE_OPACITY: f32 = 0.65;
/// Opacity for the selected row / the hovered button.
const ACTIVE_OPACITY: f32 = 1.0;

/// `GAMELOD.STR` keys, with the shipped English text as a fallback.
const HEADER_KEY: &str = "initial";
const HEADER_FALLBACK: &str = "Select a file to load.";
const EMPTY_KEY: &str = "unused";
const EMPTY_FALLBACK: &str = "< EMPTY >";
const LOAD_KEY: &str = "load";
const LOAD_FALLBACK: &str = "Load";
const DONE_KEY: &str = "done";
const DONE_FALLBACK: &str = "Done";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadGameAction {
    /// Highlight the save at this index in the visible list.
    Select(usize),
    /// Load the highlighted save.
    Load,
    /// Return to the main menu.
    Done,
}

/// Resolve the screen's widget rects from `GAMELODR.BIN`, falling back to the
/// decoded values when it is absent.
fn screen_rects(layout: Option<&[MapRect]>) -> [Rect; 4] {
    let mut rects = FALLBACK_RECTS;
    for (index, rect) in rects.iter_mut().enumerate() {
        if let Some(r) = layout.and_then(|rects| rects.get(index)) {
            *rect = Rect::new(
                r.ul_x as f32,
                r.ul_y as f32,
                r.width() as f32,
                r.height() as f32,
            );
        }
    }
    rects
}

/// Look a label up in `GAMELOD.STR`, falling back to the shipped English text
/// when the table (or the key) is missing, or the value is empty.
fn label(strings: Option<&HashMap<String, String>>, key: &str, fallback: &str) -> String {
    strings
        // The strings importer lowercases its keys.
        .and_then(|s| s.get(key))
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// How many save rows fit in the list rect.
fn visible_row_count(list: Rect) -> usize {
    (list.h / ROW_HEIGHT).floor().max(0.0) as usize
}

/// The canvas rect of the `index`-th row of the list.
fn row_rect(list: Rect, index: usize) -> Rect {
    Rect::new(
        list.x,
        list.y + index as f32 * ROW_HEIGHT,
        list.w,
        ROW_HEIGHT,
    )
}

/// Pure click resolution: on a rising press edge, map the pointer to whatever
/// it is over - a save row, "Load" (only when a save is selected), or "Done".
/// Also returns the new `last_pressed` to track for the next frame.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect; 4],
    visible_saves: usize,
    has_selection: bool,
) -> (Option<LoadGameAction>, bool) {
    let Some(p) = pointer else {
        return (None, false);
    };
    if !p.pressed || last_pressed {
        return (None, p.pressed);
    }

    let action = pointer_to_canvas(
        vec2(CANVAS_W, CANVAS_H),
        p.position,
        screen_size,
        SCALE_MODE,
    )
    .and_then(|c| {
        if has_selection && rects[LOAD_RECT_INDEX].contains(c) {
            return Some(LoadGameAction::Load);
        }
        if rects[DONE_RECT_INDEX].contains(c) {
            return Some(LoadGameAction::Done);
        }
        // Rows past the end of the save list are empty backdrop, not buttons.
        (0..visible_saves)
            .find(|index| row_rect(rects[LIST_RECT_INDEX], *index).contains(c))
            .map(LoadGameAction::Select)
    });
    (action, p.pressed)
}

pub struct LoadGameScene {
    world: World,
    scene_name: String,
    /// Saves offered by the list, resolved once when the screen opens so the
    /// rows and the click agree. Most recent first.
    saves: Vec<SaveFile>,
    /// Index into [`Self::saves`] of the highlighted row, if any.
    selected: Option<usize>,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` can map the pointer into
    /// canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
}

impl LoadGameScene {
    pub fn new() -> Self {
        let saves = all_saves();
        // Preselect the most recent save so "Load" is immediately meaningful,
        // matching the recovery the game-over screen offers.
        let selected = (!saves.is_empty()).then_some(0);

        Self {
            world: super::ui_scene_world(),
            scene_name: "load_game".to_owned(),
            saves,
            selected,
            pointer: None,
            last_pressed: false,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
        }
    }
}

impl Default for LoadGameScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for LoadGameScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = screen_rects(layout.as_deref().map(|r| r.as_slice()));
        let visible = self
            .saves
            .len()
            .min(visible_row_count(rects[LIST_RECT_INDEX]));

        self.pointer = input_context.pointer;
        let (action, last_pressed) = resolve_click(
            input_context.pointer,
            self.last_pressed,
            self.last_screen_size,
            &rects,
            visible,
            self.selected.is_some(),
        );
        self.last_pressed = last_pressed;

        match action {
            Some(LoadGameAction::Select(index)) => {
                self.selected = Some(index);
                Vec::new()
            }
            Some(LoadGameAction::Load) => self
                .selected
                .and_then(|index| self.saves.get(index))
                .map(|save| {
                    vec![Effect::GlobalEffect(GlobalEffect::Load {
                        file_name: save.path.to_string_lossy().into_owned(),
                    })]
                })
                .unwrap_or_default(),
            Some(LoadGameAction::Done) => {
                vec![Effect::GlobalEffect(GlobalEffect::ShowMainMenu)]
            }
            None => Vec::new(),
        }
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // The screen is drawn in screen space in `render_per_eye` (which has
        // the screen size); the 3D scene is empty.
        (
            Vec::new(),
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        )
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        _view: cgmath::Matrix4<f32>,
        _projection: cgmath::Matrix4<f32>,
        screen_size: Vector2<f32>,
        _options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.last_screen_size = screen_size;
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = screen_rects(layout.as_deref().map(|r| r.as_slice()));
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, LABELS_FILE);
        let strings = strings.as_deref();
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });

        canvas.text_native(
            rects[HEADER_RECT_INDEX],
            &label(strings, HEADER_KEY, HEADER_FALLBACK),
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        );

        let list = rects[LIST_RECT_INDEX];
        if self.saves.is_empty() {
            canvas
                .text_native(
                    row_rect(list, 0),
                    &label(strings, EMPTY_KEY, EMPTY_FALLBACK),
                    MENU_FONT,
                    HAlign::Center,
                    VAlign::Middle,
                )
                .opacity(DISABLED_OPACITY);
        } else {
            let visible = self.saves.len().min(visible_row_count(list));
            for (index, save) in self.saves.iter().take(visible).enumerate() {
                canvas
                    // Save names are player-authored and unbounded, so they
                    // are ellipsized to the list width rather than spilling
                    // over the Load button.
                    .text_native_fit(
                        row_rect(list, index),
                        &save.name,
                        MENU_FONT,
                        HAlign::Center,
                        VAlign::Middle,
                    )
                    .opacity(if self.selected == Some(index) {
                        ACTIVE_OPACITY
                    } else {
                        IDLE_OPACITY
                    });
            }
        }

        // "Load" is inert with nothing selected, so it reads as disabled.
        for (index, text, enabled) in [
            (
                LOAD_RECT_INDEX,
                label(strings, LOAD_KEY, LOAD_FALLBACK),
                self.selected.is_some(),
            ),
            (DONE_RECT_INDEX, label(strings, DONE_KEY, DONE_FALLBACK), true),
        ] {
            let rect = rects[index];
            let opacity = if !enabled {
                DISABLED_OPACITY
            } else if pointer_canvas.is_some_and(|p| rect.contains(p)) {
                ACTIVE_OPACITY
            } else {
                IDLE_OPACITY
            };
            canvas
                .text_native(rect, &text, MENU_FONT, HAlign::Center, VAlign::Middle)
                .opacity(opacity);
        }

        canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // The runtimes render at a 4:3 resolution, so PreserveAspect == stretch and
    // normalized coords map straight onto the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    fn pointer_at(x: f32, y: f32, pressed: bool) -> Option<Pointer2D> {
        Some(Pointer2D {
            position: vec2(x, y),
            pressed,
        })
    }

    /// Canvas point -> normalized screen point, so tests can name the widget
    /// they mean instead of a magic normalized pair.
    fn at_canvas(p: Vector2<f32>) -> Option<Pointer2D> {
        pointer_at(p.x / CANVAS_W, p.y / CANVAS_H, true)
    }

    fn click(pointer: Option<Pointer2D>, visible: usize, has_selection: bool) -> Option<LoadGameAction> {
        resolve_click(
            pointer,
            false,
            SCREEN,
            &FALLBACK_RECTS,
            visible,
            has_selection,
        )
        .0
    }

    #[test]
    fn clicking_a_row_selects_that_save() {
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        for index in 0..3 {
            assert_eq!(
                click(at_canvas(row_rect(list, index).center()), 3, false),
                Some(LoadGameAction::Select(index)),
                "row {index}"
            );
        }
    }

    #[test]
    fn clicking_past_the_last_save_does_nothing() {
        // Only two saves exist, so the third row is bare backdrop.
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        assert_eq!(click(at_canvas(row_rect(list, 2).center()), 2, false), None);
    }

    #[test]
    fn load_requires_a_selection() {
        let load = FALLBACK_RECTS[LOAD_RECT_INDEX].center();
        assert_eq!(click(at_canvas(load), 3, true), Some(LoadGameAction::Load));
        // With nothing selected the button is inert rather than loading
        // whatever happens to be first.
        assert_eq!(click(at_canvas(load), 3, false), None);
    }

    #[test]
    fn done_returns_to_the_menu_even_with_no_saves() {
        let done = FALLBACK_RECTS[DONE_RECT_INDEX].center();
        assert_eq!(click(at_canvas(done), 0, false), Some(LoadGameAction::Done));
    }

    #[test]
    fn held_press_does_not_re_activate() {
        let done = FALLBACK_RECTS[DONE_RECT_INDEX].center();
        let (action, last) = resolve_click(
            pointer_at(done.x / CANVAS_W, done.y / CANVAS_H, true),
            true,
            SCREEN,
            &FALLBACK_RECTS,
            0,
            false,
        );
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn no_pointer_means_no_action() {
        let (action, last) = resolve_click(None, true, SCREEN, &FALLBACK_RECTS, 3, true);
        assert_eq!(action, None);
        assert!(!last);
    }

    #[test]
    fn the_vanilla_list_holds_fourteen_rows() {
        // 290px of list at a 20px row pitch. Guards the row geometry that the
        // hit-testing and the render loop share.
        assert_eq!(visible_row_count(FALLBACK_RECTS[LIST_RECT_INDEX]), 14);
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        assert_eq!(row_rect(list, 0), Rect::new(261.0, 54.0, 202.0, 20.0));
        assert_eq!(row_rect(list, 13), Rect::new(261.0, 314.0, 202.0, 20.0));
        // Every row stays inside the authored list rect.
        assert!(row_rect(list, 13).y + ROW_HEIGHT <= list.y + list.h);
    }

    #[test]
    fn screen_rects_prefer_layout_and_fall_back() {
        let layout: Vec<MapRect> = (0..4)
            .map(|i| MapRect::new(10, i * 70, 110, i * 70 + 50))
            .collect();
        let rects = screen_rects(Some(&layout));
        assert_eq!(rects[HEADER_RECT_INDEX], Rect::new(10.0, 0.0, 100.0, 50.0));
        assert_eq!(rects[DONE_RECT_INDEX], Rect::new(10.0, 210.0, 100.0, 50.0));
        // Without a layout: the decoded vanilla GAMELODR.BIN geometry.
        assert_eq!(screen_rects(None), FALLBACK_RECTS);
    }

    /// The body of the shipped `res/intrface/GAMELOD.STR`, verbatim. Kept here
    /// so a key rename - ours or the data's - fails a test rather than silently
    /// falling back to the English constants, which is invisible at runtime on
    /// an English install because the two agree.
    const SHIPPED_GAMELOD_STR: &str = concat!(
        "delete:\"Delete\"\n",
        "failed:\"Load Failed\"\n",
        "loading:\"Loading...\"\n",
        "initial:\"Select a file to load.\"\n",
        "unused:\"< EMPTY >\"\n",
        "done:\"Done\"\n",
        "load:\"Load\"\n",
    );

    #[test]
    fn every_label_key_resolves_against_the_shipped_string_table() {
        let lines: Vec<String> = SHIPPED_GAMELOD_STR
            .lines()
            .map(|l| l.to_owned())
            .collect();
        let strings = dark::importers::parse_strings(&lines);

        for (key, fallback) in [
            (HEADER_KEY, HEADER_FALLBACK),
            (EMPTY_KEY, EMPTY_FALLBACK),
            (LOAD_KEY, LOAD_FALLBACK),
            (DONE_KEY, DONE_FALLBACK),
        ] {
            assert!(strings.contains_key(key), "GAMELOD.STR has no key '{key}'");
            // The shipped text is what we fall back to, so a drift in either
            // direction shows up here.
            assert_eq!(label(Some(&strings), key, "!unused!"), fallback);
        }
    }

    #[test]
    fn labels_fall_back_without_a_string_table() {
        assert_eq!(label(None, LOAD_KEY, LOAD_FALLBACK), "Load");
        // An empty shipped value must not blank the widget.
        let strings = HashMap::from([(DONE_KEY.to_owned(), String::new())]);
        assert_eq!(label(Some(&strings), DONE_KEY, DONE_FALLBACK), "Done");
    }
}
