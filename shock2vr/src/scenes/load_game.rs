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
use dark::importers::STRINGS_IMPORTER;
#[cfg(test)]
use dark::map::MapRect;
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
    input_context::InputContext,
    mission::GlobalContext,
    save_load::{SaveFile, all_saves},
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{FrontendMenu, HAlign, Rect, ScaleMode, UiCanvas, VAlign, resolve_menu_label},
};

#[cfg(test)]
use crate::{
    input_context::Pointer2D,
    ui::{
        resolve_click_at as shell_resolve_click_at, resolve_flat_click, resolve_menu_rects,
        vr_frontend_pointer_pass,
    },
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

/// Row pitch inside the list rect, in canvas pixels. Rows are hit-tested at
/// this pitch, so it also fixes the click targets.
///
/// The screen was authored around `METAFONT.FON`'s 20px cell, but save names
/// draw in 11px [`LIST_FONT`], so a 20px pitch is mostly whitespace - and it
/// costs a slot, because only 13 such rows clear [`FIELD_TOP_Y`]. 19px is the
/// largest pitch that fits the shipped list's full 14, which matters while
/// there is no scrolling: the last row is the last reachable save (#928).
const ROW_HEIGHT: f32 = 19.0;

/// Canvas y where `GAMELOD.PCX` starts painting a bordered field, decoded from
/// the art (its border rows are 323, 325, 343 and 345). It is the save
/// screen's name-entry box - `GAMESAVR.BIN` and `GAMELODR.BIN` are
/// byte-identical and the two screens share this art - so on the load screen
/// it is inert decoration that rows must still clear.
///
/// This is an absolute position in the backdrop, not an offset within the list
/// rect: rows have to stop here no matter where the layout file puts the list.
/// Naive `290 / 20` ignores it entirely, yielding 14 rows and drawing the last
/// one straight through the field's border.
const FIELD_TOP_Y: f32 = 323.0;

/// Horizontal padding for a save name, applied to both edges of the row: the
/// left so a left-aligned name clears the list panel's edge, the right so an
/// ellipsized one stops short of it rather than running flush.
const LIST_TEXT_INSET: f32 = 8.0;

/// Save names are drawn in the small in-game font (11px) rather than the
/// screen's 20px `METAFONT.FON`, so the list reads as data under the heavier
/// header and buttons.
const LIST_FONT: &str = "mainfont.fon";

/// Opacity for a row or button that cannot be acted on.
const DISABLED_OPACITY: f32 = 0.3;
/// Opacity for an actionable, unhighlighted element.
const IDLE_OPACITY: f32 = 0.65;
/// Opacity for the selected row / the hovered button.
const ACTIVE_OPACITY: f32 = 1.0;

/// `GAMELOD.STR` keys, with the shipped English text as a fallback.
const HEADER_KEY: &str = "initial";
const HEADER_FALLBACK: &str = "Select a file to load.";
const FAILED_KEY: &str = "failed";
const FAILED_FALLBACK: &str = "Load Failed";
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LoadGameStatus {
    #[default]
    Initial,
    Failed,
}

/// Resolve the screen's widget rects from `GAMELODR.BIN`, falling back to the
/// decoded values when it is absent.
#[cfg(test)]
fn screen_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    resolve_menu_rects(layout, &FALLBACK_RECTS)
}

/// Look a label up in `GAMELOD.STR`, falling back to the shipped English text
/// when the table (or the key) is missing, or the value is empty.
fn label(strings: Option<&HashMap<String, String>>, key: &str, fallback: &str) -> String {
    resolve_menu_label(strings, key, fallback)
}

/// How many save rows fit in the list rect, stopping at the backdrop's painted
/// field (see [`FIELD_TOP_Y`]) when the rect runs past it.
fn visible_row_count(list: Rect) -> usize {
    let usable = (list.y + list.h).min(FIELD_TOP_Y) - list.y;
    (usable / ROW_HEIGHT).floor().max(0.0) as usize
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

/// The rect a row's save name is drawn in: the row, inset on both sides so the
/// name clears the panel edges. Hit-testing still uses the full [`row_rect`],
/// so the inset never costs a click.
fn row_text_rect(list: Rect, index: usize) -> Rect {
    let row = row_rect(list, index);
    Rect::new(
        row.x + LIST_TEXT_INSET,
        row.y,
        (row.w - 2.0 * LIST_TEXT_INSET).max(0.0),
        row.h,
    )
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
#[cfg(test)]
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
    visible_saves: usize,
    has_selection: bool,
) -> (Option<LoadGameAction>, bool) {
    shell_resolve_click_at(point, pressed, last_pressed, |point| {
        hit(point, rects, visible_saves, has_selection)
    })
}

/// Pure click resolution: on a rising press edge, map the pointer to whatever
/// it is over - a save row, "Load" (only when a save is selected), or "Done".
/// Also returns the new `last_pressed` to track for the next frame.
#[cfg(test)]
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
    visible_saves: usize,
    has_selection: bool,
) -> (Option<LoadGameAction>, bool, Option<Vector2<f32>>) {
    resolve_flat_click(
        pointer,
        last_pressed,
        screen_size,
        vec2(CANVAS_W, CANVAS_H),
        SCALE_MODE,
        |point| hit(point, rects, visible_saves, has_selection),
    )
}

/// What is at a canvas point: a save row, "Load" (only with a selection), or
/// "Done". Shared by the click and the rollover sound so the two always agree.
fn hit(
    point: Vector2<f32>,
    rects: &[Rect],
    visible_saves: usize,
    has_selection: bool,
) -> Option<LoadGameAction> {
    if has_selection && rects[LOAD_RECT_INDEX].contains(point) {
        return Some(LoadGameAction::Load);
    }
    if rects[DONE_RECT_INDEX].contains(point) {
        return Some(LoadGameAction::Done);
    }
    // Rows past the end of the save list are empty backdrop, not buttons.
    (0..visible_saves)
        .find(|index| row_rect(rects[LIST_RECT_INDEX], *index).contains(point))
        .map(LoadGameAction::Select)
}

pub struct LoadGameScene {
    world: World,
    scene_name: String,
    /// Saves offered by the list, resolved once when the screen opens so the
    /// rows and the click agree. Most recent first.
    saves: Vec<SaveFile>,
    /// Index into [`Self::saves`] of the highlighted row, if any.
    selected: Option<usize>,
    /// Result of the latest load attempt. The header is the screen's authored
    /// status line (`initial` / `failed` in `GAMELOD.STR`).
    status: LoadGameStatus,
    menu: FrontendMenu<LoadGameAction>,
}

impl LoadGameScene {
    pub fn new() -> Self {
        Self::from_saves(all_saves())
    }

    fn from_saves(saves: Vec<SaveFile>) -> Self {
        // Preselect the most recent save so "Load" is immediately meaningful,
        // matching the recovery the game-over screen offers.
        let selected = (!saves.is_empty()).then_some(0);

        Self {
            world: super::ui_scene_world(),
            scene_name: "load_game".to_owned(),
            saves,
            selected,
            status: LoadGameStatus::Initial,
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
        }
    }

    fn header_label(&self, strings: Option<&HashMap<String, String>>) -> String {
        match self.status {
            LoadGameStatus::Initial => label(strings, HEADER_KEY, HEADER_FALLBACK),
            LoadGameStatus::Failed => label(strings, FAILED_KEY, FAILED_FALLBACK),
        }
    }

    fn apply_action(&mut self, action: Option<LoadGameAction>) -> Vec<Effect> {
        match action {
            Some(LoadGameAction::Select(index)) => {
                self.selected = Some(index);
                // The previous failure belonged to the old selection.
                self.status = LoadGameStatus::Initial;
                Vec::new()
            }
            Some(LoadGameAction::Load) => {
                // Clear stale feedback while the retry is dispatched. A
                // synchronous failure sets it again through `on_load_failed`;
                // success replaces this scene.
                self.status = LoadGameStatus::Initial;
                self.selected
                    .and_then(|index| self.saves.get(index))
                    .map(|save| {
                        vec![Effect::GlobalEffect(GlobalEffect::Load {
                            file_name: save.path.to_string_lossy().into_owned(),
                        })]
                    })
                    .unwrap_or_default()
            }
            Some(LoadGameAction::Done) => {
                vec![Effect::GlobalEffect(GlobalEffect::ShowMainMenu)]
            }
            None => Vec::new(),
        }
    }
}

impl LoadGameScene {
    /// The screen, described once. Screen-space and world-space presentation
    /// differ only in how this canvas is rendered, so the two can never drift
    /// apart in layout, labels, or which widgets look actionable.
    ///
    /// `pointer_canvas` is the hover position in canvas pixels, whatever
    /// produced it - the mouse or a VR controller ray.
    fn build_canvas(
        &self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, LABELS_FILE);
        let strings = strings.as_deref();

        canvas.text_native(
            rects[HEADER_RECT_INDEX],
            &self.header_label(strings),
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        );

        let list = rects[LIST_RECT_INDEX];
        if self.saves.is_empty() {
            // Deliberately header-styled rather than row-styled: this is a
            // message about the list, not an entry in it, so it stays centered
            // in MENU_FONT while real saves read as left-aligned data.
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
                        row_text_rect(list, index),
                        &save.name,
                        LIST_FONT,
                        HAlign::Left,
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
            (
                DONE_RECT_INDEX,
                label(strings, DONE_KEY, DONE_FALLBACK),
                true,
            ),
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

        canvas
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
        game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let visible = self
            .saves
            .len()
            .min(visible_row_count(rects[LIST_RECT_INDEX]));

        // Only a save the player can actually see is loadable. The constructor
        // preselects row 0 from `saves` alone, which a layout too short to show
        // a single row would otherwise turn into a "Load" for an invisible one.
        // Clamped on the field rather than into a local, so `build_canvas` draws
        // "Load" disabled in exactly the cases the click rejects it.
        self.selected = self.selected.filter(|index| *index < visible);
        let selected = self.selected;

        let action = self.menu.update(
            time.elapsed,
            input_context,
            game_options.presentation_mode,
            |point| hit(point, &rects, visible, selected.is_some()),
            // Rows are deliberately silent: they highlight on selection
            // rather than hover, so only visible hover feedback makes noise.
            |point| hit(point, &rects, 0, selected.is_some()),
        );

        self.apply_action(action)
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

    fn on_load_failed(&mut self) {
        self.status = LoadGameStatus::Failed;
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

    fn click(
        pointer: Option<Pointer2D>,
        visible: usize,
        has_selection: bool,
    ) -> Option<LoadGameAction> {
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
        let (action, last, _) = resolve_click(
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
    fn pointer_loss_preserves_a_held_press() {
        let (action, last, _) = resolve_click(None, true, SCREEN, &FALLBACK_RECTS, 3, true);
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn the_vanilla_list_holds_fourteen_rows_clear_of_the_field() {
        // Guards the row geometry that the hit-testing and the render loop
        // share: all 14 of the shipped list's rows are reachable, and none of
        // them touches the backdrop's painted field.
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        assert_eq!(visible_row_count(list), 14);
        assert_eq!(row_rect(list, 0), Rect::new(261.0, 54.0, 202.0, 19.0));
        assert_eq!(row_rect(list, 13), Rect::new(261.0, 301.0, 202.0, 19.0));
        // Every row stays inside the authored list rect...
        assert!(row_rect(list, 13).y + ROW_HEIGHT <= list.y + list.h);
        // ...and clears the painted field.
        assert!(row_rect(list, 13).y + ROW_HEIGHT <= FIELD_TOP_Y);
        // A 15th would not, so the count stops where the art does.
        assert!(row_rect(list, 14).y + ROW_HEIGHT > FIELD_TOP_Y);
        // The pitch is the largest that fits all 14 - one more pixel loses a
        // row, which is what a naive 20px pitch did.
        assert!(list.y + 14.0 * (ROW_HEIGHT + 1.0) > FIELD_TOP_Y);
    }

    #[test]
    fn the_row_count_tracks_the_field_position_not_the_rect_height() {
        // The field is at a fixed place in the backdrop, so the row count has
        // to follow the list's position, not just its height. Reserving a
        // fixed slice of `h` would get both of these wrong.
        //
        // A list clear of the field uses its full height...
        let above = Rect::new(261.0, 20.0, 202.0, 290.0);
        assert!(above.y + above.h <= FIELD_TOP_Y);
        assert_eq!(visible_row_count(above), 15);
        // ...while one running past it stops at the field, however tall it is.
        let over = Rect::new(261.0, 54.0, 202.0, 350.0);
        assert_eq!(visible_row_count(over), 14);
        assert!(row_rect(over, 13).y + ROW_HEIGHT <= FIELD_TOP_Y);
        // A list starting below the field shows nothing rather than underflowing.
        assert_eq!(visible_row_count(Rect::new(261.0, 400.0, 202.0, 60.0)), 0);
    }

    #[test]
    fn row_text_is_inset_from_the_left_but_the_whole_row_stays_clickable() {
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        let row = row_rect(list, 0);
        let text = row_text_rect(list, 0);
        assert_eq!(text, Rect::new(269.0, 54.0, 186.0, ROW_HEIGHT));
        // Inset on both edges, so an ellipsized name stops short of the panel.
        assert_eq!(text.x - row.x, LIST_TEXT_INSET);
        assert_eq!((row.x + row.w) - (text.x + text.w), LIST_TEXT_INSET);
        // A click in either inset gap still selects the row.
        for x in [list.x + 2.0, list.x + list.w - 2.0] {
            assert_eq!(
                click(at_canvas(vec2(x, list.y + 10.0)), 3, false),
                Some(LoadGameAction::Select(0))
            );
        }
    }

    /// The panel the anchor places on scene entry from the default head pose -
    /// what `update` would have hit-tested against on the screen's first frame.
    fn test_panel() -> crate::ui::WorldPanel {
        crate::ui::test_support::test_panel()
    }

    /// A hand aimed at a canvas point on this screen's VR panel.
    fn hand_aimed_at(point: Vector2<f32>, trigger: f32) -> crate::input_context::Hand {
        crate::ui::test_support::hand_aimed_at(vec2(CANVAS_W, CANVAS_H), point, trigger)
    }

    fn vr_input(hand: crate::input_context::Hand) -> InputContext {
        InputContext {
            right_hand: hand,
            ..InputContext::default()
        }
    }

    #[test]
    fn a_vr_ray_can_press_done() {
        let done = FALLBACK_RECTS[DONE_RECT_INDEX].center();
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(done, 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        let point = point.expect("the ray should land on the panel");
        assert!(FALLBACK_RECTS[DONE_RECT_INDEX].contains(point));
        assert!(pressed);
        assert_eq!(
            resolve_click_at(Some(point), pressed, false, &FALLBACK_RECTS, 0, false).0,
            Some(LoadGameAction::Done)
        );
    }

    #[test]
    fn a_vr_ray_can_select_a_save_row_and_load_it() {
        let list = FALLBACK_RECTS[LIST_RECT_INDEX];
        let row = row_rect(list, 2).center();
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(row, 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        let point = point.expect("the ray should land on the panel");
        assert_eq!(
            resolve_click_at(Some(point), pressed, false, &FALLBACK_RECTS, 3, false).0,
            Some(LoadGameAction::Select(2))
        );

        // With a selection, the same rig over "Load" performs the load.
        let load = FALLBACK_RECTS[LOAD_RECT_INDEX].center();
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(load, 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        assert_eq!(
            resolve_click_at(point, pressed, false, &FALLBACK_RECTS, 3, true).0,
            Some(LoadGameAction::Load)
        );
    }

    #[test]
    fn a_vr_press_held_across_frames_clicks_once() {
        let point = Some(FALLBACK_RECTS[DONE_RECT_INDEX].center());
        let (action, last) = resolve_click_at(point, true, false, &FALLBACK_RECTS, 0, false);
        assert_eq!(action, Some(LoadGameAction::Done));
        assert_eq!(
            resolve_click_at(point, true, last, &FALLBACK_RECTS, 0, false).0,
            None
        );
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
        let lines: Vec<String> = SHIPPED_GAMELOD_STR.lines().map(|l| l.to_owned()).collect();
        let strings = dark::importers::parse_strings(&lines);

        for (key, fallback) in [
            (HEADER_KEY, HEADER_FALLBACK),
            (FAILED_KEY, FAILED_FALLBACK),
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

    fn scene_with_save() -> LoadGameScene {
        LoadGameScene::from_saves(vec![SaveFile {
            name: "corrupt".to_owned(),
            path: std::path::PathBuf::from("corrupt.sav"),
            modified: std::time::SystemTime::UNIX_EPOCH,
        }])
    }

    #[test]
    fn failed_load_uses_the_shipped_status_label() {
        let lines: Vec<String> = SHIPPED_GAMELOD_STR.lines().map(str::to_owned).collect();
        let strings = dark::importers::parse_strings(&lines);
        let mut scene = scene_with_save();

        assert_eq!(scene.header_label(Some(&strings)), HEADER_FALLBACK);
        scene.on_load_failed();
        assert_eq!(scene.header_label(Some(&strings)), FAILED_FALLBACK);
    }

    #[test]
    fn failure_state_clears_for_selection_and_retry_and_is_fresh_on_reentry() {
        let mut scene = scene_with_save();
        scene.menu.set_last_pressed(true);

        scene.on_load_failed();
        assert_eq!(scene.status, LoadGameStatus::Failed);
        assert!(
            scene.menu.last_pressed(),
            "load feedback must not rearm a held press"
        );

        scene.apply_action(Some(LoadGameAction::Select(0)));
        assert_eq!(scene.status, LoadGameStatus::Initial);

        scene.on_load_failed();
        let effects = scene.apply_action(Some(LoadGameAction::Load));
        assert_eq!(scene.status, LoadGameStatus::Initial);
        assert!(matches!(
            effects.as_slice(),
            [Effect::GlobalEffect(GlobalEffect::Load { file_name })]
                if file_name == "corrupt.sav"
        ));

        // A failed retry reports the same state again, while leaving and
        // re-entering constructs a new prompt rather than preserving it.
        scene.on_load_failed();
        assert_eq!(scene.status, LoadGameStatus::Failed);
        assert_eq!(scene_with_save().status, LoadGameStatus::Initial);
    }
}
