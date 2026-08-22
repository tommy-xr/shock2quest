//! Flatscreen main menu.
//!
//! A minimal `GameScene` that draws the original `MAIN.PCX` backdrop with the
//! six mouse-clickable menu entries, described on the shared [`UiCanvas`]. It
//! reads `InputContext::pointer` (normalized screen coords) and emits a
//! `GlobalEffect` on click: New Game -> `TransitionLevel` into the first
//! mission, Quit -> `Quit`. Entries the port does not implement yet are drawn
//! dimmed and ignore clicks.
//!
//! Everything the screen needs is read from the shipped data rather than
//! hardcoded: labels from `MAIN.STR`, button rects from `MAINR.BIN`. That
//! matters beyond fidelity - the community mod layers (SCP) ship a redrawn
//! backdrop *with a retuned `*R.BIN`*, so a hardcoded rect is wrong on a
//! modded install.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slice 3).

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
#[cfg(test)]
use dark::map::MapRect;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
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
    ui::{
        FrontendMenu, FrontendMenuItem, HAlign, Rect, ScaleMode, UiCanvas, VAlign, hit_menu_item,
    },
};

#[cfg(test)]
use crate::{
    input_context::Pointer2D,
    ui::{
        WorldPanel, resolve_click_at as shell_resolve_click_at, resolve_flat_click,
        resolve_menu_labels, resolve_menu_rects, vr_frontend_pointer_pass,
    },
};
#[cfg(test)]
use std::collections::HashMap;

/// Mission loaded when the player chooses "New Game".
const NEW_GAME_MISSION: &str = "earth.mis";

/// The menu is authored on the original 640x480 `MAIN.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// The original menu draws its labels with the default GUI style font -
/// METAFONT.FON (`res/intrface/`), a 20px-cell antialias-16 display face.
const MENU_FONT: &str = "metafont.fon";
/// Original widget layout for `MAIN.PCX` - LTRB rects for the six buttons
/// (top to bottom) plus the corner logo (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "MAINR.BIN";
/// Original label strings for this screen, keyed by [`MenuItem::string_key`].
const LABELS_FILE: &str = "MAIN.STR";
/// The 4:3 menu art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Opacity for an entry the port has not implemented yet.
const DISABLED_OPACITY: f32 = 0.3;
/// Opacity for an implemented entry the pointer is not over.
const IDLE_OPACITY: f32 = 0.6;
/// Opacity for the entry under the pointer.
const HOVER_OPACITY: f32 = 1.0;

// Fallback button geometry, used only when `MAINR.BIN` is missing: the decoded
// vanilla values - a column of six 179x60 buttons at x=400 on a 76px pitch.
const FALLBACK_BUTTON_X: f32 = 400.0;
const FALLBACK_BUTTON_TOP: f32 = 20.0;
const FALLBACK_BUTTON_W: f32 = 179.0;
const FALLBACK_BUTTON_H: f32 = 60.0;
const FALLBACK_BUTTON_PITCH: f32 = 76.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    NewGame,
    LoadGame,
    Developer,
    Quit,
}

// MAIN.PCX (native 640x480) has a vertical stack of six buttons down the right
// side. This list is in screen order, top to bottom, so an item's index is also
// its rect index in `MAINR.BIN`. Labels are centered in the button rect.
//
// (`MAIN.STR` itself lists the keys in reverse screen order; the same reversal
// holds for `SIM.STR` against the known pause-menu order.)
const MENU_ITEMS: &[FrontendMenuItem<MenuAction>] = &[
    FrontendMenuItem {
        string_key: "new_game",
        fallback_label: "New Game",
        action: Some(MenuAction::NewGame),
        label_override: None,
    },
    FrontendMenuItem {
        string_key: "load_game",
        fallback_label: "Load Game",
        action: Some(MenuAction::LoadGame),
        label_override: None,
    },
    // The Options slot hosts the Developer screen while no real options
    // screen exists (see `MenuItem::label_override`).
    FrontendMenuItem {
        string_key: "options",
        fallback_label: "Options",
        action: Some(MenuAction::Developer),
        label_override: Some("Developer"),
    },
    FrontendMenuItem {
        string_key: "credits",
        fallback_label: "Credits",
        action: None,
        label_override: None,
    },
    FrontendMenuItem {
        string_key: "intro",
        fallback_label: "Intro",
        action: None,
        label_override: None,
    },
    FrontendMenuItem {
        string_key: "quit",
        fallback_label: "Quit",
        action: Some(MenuAction::Quit),
        label_override: None,
    },
];

const FALLBACK_RECTS: [Rect; 6] = [
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP + FALLBACK_BUTTON_PITCH,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP + 2.0 * FALLBACK_BUTTON_PITCH,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP + 3.0 * FALLBACK_BUTTON_PITCH,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP + 4.0 * FALLBACK_BUTTON_PITCH,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
    Rect::new(
        FALLBACK_BUTTON_X,
        FALLBACK_BUTTON_TOP + 5.0 * FALLBACK_BUTTON_PITCH,
        FALLBACK_BUTTON_W,
        FALLBACK_BUTTON_H,
    ),
];

/// Resolve each menu item's canvas rect from the `MAINR.BIN` layout (falling
/// back to the vanilla geometry if it's absent). Parallel to [`MENU_ITEMS`].
#[cfg(test)]
fn menu_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    resolve_menu_rects(layout, &FALLBACK_RECTS)
}

/// Resolve each menu item's label from `MAIN.STR`, falling back to the shipped
/// English text when the string table is absent. Parallel to [`MENU_ITEMS`].
#[cfg(test)]
fn menu_labels(strings: Option<&HashMap<String, String>>) -> Vec<String> {
    resolve_menu_labels(strings, MENU_ITEMS)
}

/// Where the hands are pointing on the menu panel. The rule is the frontend's,
/// not this screen's, so it lives in [`vr_frontend_pointer_pass`].
#[cfg(test)]
fn vr_pointer(input_context: &InputContext, panel: &WorldPanel) -> crate::ui::FrontendPointerPass {
    vr_frontend_pointer_pass(input_context, vec2(CANVAS_W, CANVAS_H), panel)
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
#[cfg(test)]
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
) -> (Option<MenuAction>, bool) {
    shell_resolve_click_at(point, pressed, last_pressed, |point| hit(point, rects))
}

/// The menu entry at a canvas point, if any. Shared by the click and the
/// rollover sound so the two can never disagree about where an entry is.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<MenuAction> {
    hit_menu_item(point, MENU_ITEMS, rects, |_| true)
}

/// Pure click resolution: on a rising press edge over an implemented item
/// (`rects` is parallel to [`MENU_ITEMS`]), return its action. Also returns the
/// new `last_pressed` to track for the next frame.
#[cfg(test)]
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<MenuAction>, bool, Option<Vector2<f32>>) {
    resolve_flat_click(
        pointer,
        last_pressed,
        screen_size,
        vec2(CANVAS_W, CANVAS_H),
        SCALE_MODE,
        |point| hit(point, rects),
    )
}

pub struct MainMenuScene {
    world: World,
    scene_name: String,
    menu: FrontendMenu<MenuAction>,
}

impl MainMenuScene {
    pub fn new() -> Self {
        let world = super::ui_scene_world();

        Self {
            world,
            scene_name: "main_menu".to_owned(),
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
        }
    }
}

impl MainMenuScene {
    /// The menu, described once. Screen-space and world-space presentation
    /// differ only in how this canvas is rendered, so the two can never drift
    /// apart in layout, labels, or which entries look actionable.
    ///
    /// `pointer_canvas` is the hover position in canvas pixels, whatever
    /// produced it - the mouse or a VR controller ray.
    fn build_canvas(
        &self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));

        // Full-screen backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), "MAIN.PCX");

        // Menu items, centered in their button and brighter when hovered.
        // Button rects come from the original `MAINR.BIN` layout and labels
        // from `MAIN.STR` (both cached by the asset cache after first load).
        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let labels = self.menu.labels(asset_cache, LABELS_FILE, MENU_ITEMS);
        for ((item, rect), label) in MENU_ITEMS.iter().zip(&rects).zip(&labels) {
            let opacity = if item.action.is_none() {
                DISABLED_OPACITY
            } else if pointer_canvas.is_some_and(|pp| rect.contains(pp)) {
                HOVER_OPACITY
            } else {
                IDLE_OPACITY
            };
            canvas
                .text_native(*rect, label, MENU_FONT, HAlign::Center, VAlign::Middle)
                .opacity(opacity);
        }
        canvas
    }
}

impl Default for MainMenuScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for MainMenuScene {
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
        let action = self.menu.update(
            time.elapsed,
            input_context,
            game_options.presentation_mode,
            |point| hit(point, &rects),
            |point| hit(point, &rects),
        );

        match action {
            Some(MenuAction::NewGame) => {
                vec![Effect::GlobalEffect(GlobalEffect::TransitionLevel {
                    level_file: NEW_GAME_MISSION.to_owned(),
                    loc: None,
                    entities_to_trigger: vec![],
                    vitals_transition:
                        crate::scripts::PlayerVitalsTransition::InitializeFromDestination,
                })]
            }
            Some(MenuAction::LoadGame) => {
                vec![Effect::GlobalEffect(GlobalEffect::ShowLoadGame)]
            }
            Some(MenuAction::Developer) => {
                vec![Effect::GlobalEffect(GlobalEffect::ShowDeveloper)]
            }
            Some(MenuAction::Quit) => vec![Effect::GlobalEffect(GlobalEffect::Quit)],
            None => Vec::new(),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_at(x: f32, y: f32, pressed: bool) -> Option<Pointer2D> {
        Some(Pointer2D {
            position: vec2(x, y),
            pressed,
        })
    }

    // The runtimes render at a 4:3 resolution, so PreserveAspect == stretch and
    // normalized coords map straight to the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    #[test]
    fn rising_edge_over_new_game_activates_it() {
        // Press edge over the top button (normalized ~ canvas (512, 37)).
        let (action, last, _) = resolve_click(
            pointer_at(0.8, 0.078, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, Some(MenuAction::NewGame));
        assert!(last);
    }

    #[test]
    fn rising_edge_over_quit_activates_it() {
        let (action, _, _) = resolve_click(
            pointer_at(0.8, 0.870, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, Some(MenuAction::Quit));
    }

    #[test]
    fn held_press_does_not_re_activate() {
        // Already pressed last frame -> no new activation even over an item.
        let (action, last, _) = resolve_click(
            pointer_at(0.8, 0.078, true),
            true,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn click_outside_items_does_nothing() {
        let (action, _, _) = resolve_click(
            pointer_at(0.05, 0.05, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, None);
    }

    #[test]
    fn a_press_held_from_the_previous_scene_does_not_click() {
        // The load screen's "Done" button center (canvas 574.5, 436) falls
        // inside this menu's "Quit" rect, so a press still held when that
        // screen swaps to the menu would otherwise quit the game outright.
        let rects = menu_rects(None);
        assert!(
            rects[5].contains(vec2(574.5, 436.0)),
            "this test is only meaningful while the rects overlap"
        );

        let held = pointer_at(574.5 / CANVAS_W, 436.0 / CANVAS_H, true);

        // First frame after the swap: the press is held, not new.
        let (action, last, _) = resolve_click(held, true, SCREEN, &rects);
        assert_eq!(action, None, "a carried-over press must not activate Quit");

        // Releasing and pressing again is a real click.
        let (_, last, _) = resolve_click(
            pointer_at(574.5 / CANVAS_W, 436.0 / CANVAS_H, false),
            last,
            SCREEN,
            &rects,
        );
        let (action, _, _) = resolve_click(held, last, SCREEN, &rects);
        assert_eq!(action, Some(MenuAction::Quit));
    }

    /// The panel the anchor places on scene entry from the default head pose -
    /// what `update` would have hit-tested against on the menu's first frame.
    fn test_panel() -> WorldPanel {
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
    fn a_vr_ray_maps_onto_the_menu_items() {
        // Aim at the center of "New Game" (rect 0) and pull the trigger.
        let rects = menu_rects(None);
        let target = rects[0].center();
        let pass = vr_pointer(&vr_input(hand_aimed_at(target, 1.0)), &test_panel());
        let (point, pressed) = (pass.point(), pass.pressed);
        let point = point.expect("the ray should land on the panel");
        assert!(
            rects[0].contains(point),
            "expected the ray to land in the New Game rect, got {point:?}"
        );
        assert!(pressed, "a fully pulled trigger is a press");

        let (action, _) = resolve_click_at(Some(point), pressed, false, &rects);
        assert_eq!(action, Some(MenuAction::NewGame));
    }

    #[test]
    fn the_left_hand_can_drive_the_menu_too() {
        // The right hand sits at its default pose, which points straight at the
        // panel center - so the left hand only wins because its trigger is the
        // one being held.
        let rects = menu_rects(None);
        let input = InputContext {
            left_hand: hand_aimed_at(rects[5].center(), 1.0),
            ..InputContext::default()
        };
        let pass = vr_pointer(&input, &test_panel());
        let (point, pressed) = (pass.point(), pass.pressed);
        let point = point.expect("the left hand should land on the panel");
        assert!(rects[5].contains(point));
        assert_eq!(
            resolve_click_at(Some(point), pressed, false, &rects).0,
            Some(MenuAction::Quit)
        );
    }

    #[test]
    fn a_light_vr_trigger_is_not_a_press() {
        let rects = menu_rects(None);
        let pass = vr_pointer(
            &vr_input(hand_aimed_at(rects[0].center(), 0.2)),
            &test_panel(),
        );
        let (_, pressed) = (pass.point(), pass.pressed);
        assert!(!pressed, "a barely-touched trigger must not click");
    }

    #[test]
    fn aiming_away_from_the_vr_panel_yields_no_point() {
        // Rotated 180 degrees: pointing behind the player, away from the panel.
        // Both hands must aim away - in VR both are always posed, so leaving
        // one at its default would have it pointing straight at the panel.
        let away = || crate::ui::test_support::hand_aimed_away(1.0);
        let input = InputContext {
            right_hand: away(),
            left_hand: away(),
            ..InputContext::default()
        };
        let pass = vr_pointer(&input, &test_panel());
        let (point, pressed) = (pass.point(), pass.pressed);
        assert_eq!(point, None);
        // The trigger is still reported so the held press is consumed rather
        // than becoming a fresh edge when the ray swings back onto a button.
        assert!(pressed);
        assert_eq!(
            resolve_click_at(point, pressed, false, &menu_rects(None)).0,
            None
        );
    }

    #[test]
    fn a_vr_press_held_across_frames_clicks_once() {
        let rects = menu_rects(None);
        let point = Some(rects[0].center());
        let (action, last) = resolve_click_at(point, true, false, &rects);
        assert_eq!(action, Some(MenuAction::NewGame));
        // Still held on the next frame: no repeat.
        assert_eq!(resolve_click_at(point, true, last, &rects).0, None);
    }

    #[test]
    fn pointer_loss_does_not_rearm_a_held_press() {
        let (action, last, _) = resolve_click(None, true, SCREEN, &menu_rects(None));
        assert_eq!(action, None);
        assert!(last, "a missing pointer is not evidence of a release");
    }

    #[test]
    fn rising_edge_over_load_game_activates_it() {
        // Rect 1 is "Load Game": canvas y 96..156, so y ~0.26 sits inside it.
        let rects = menu_rects(None);
        assert!(rects[1].contains(vec2(512.0, 126.0)));
        let (action, _, _) = resolve_click(pointer_at(0.8, 0.2625, true), false, SCREEN, &rects);
        assert_eq!(action, Some(MenuAction::LoadGame));
    }

    #[test]
    fn rising_edge_over_the_developer_slot_activates_it() {
        // Rect 2 was the inert "Options" slot; it now hosts the Developer
        // screen (canvas y 172..232).
        let rects = menu_rects(None);
        assert!(rects[2].contains(vec2(512.0, 202.0)));
        let (action, _, _) = resolve_click(pointer_at(0.8, 0.4208, true), false, SCREEN, &rects);
        assert_eq!(action, Some(MenuAction::Developer));
    }

    #[test]
    fn click_over_an_unimplemented_item_does_nothing() {
        // Rect 3 is "Credits", still unimplemented: canvas y 248..308.
        let rects = menu_rects(None);
        assert!(rects[3].contains(vec2(512.0, 278.0)));
        let (action, _, _) = resolve_click(pointer_at(0.8, 0.5792, true), false, SCREEN, &rects);
        assert_eq!(action, None);
    }

    #[test]
    fn menu_rects_prefer_layout_and_fall_back() {
        // Item index is the rect index: six buttons, top to bottom.
        let layout: Vec<MapRect> = (0..7)
            .map(|i| MapRect::new(10, i * 70, 110, i * 70 + 50))
            .collect();
        let rects = menu_rects(Some(&layout));
        assert_eq!(rects.len(), MENU_ITEMS.len());
        assert_eq!(rects[0], Rect::new(10.0, 0.0, 100.0, 50.0));
        assert_eq!(rects[5], Rect::new(10.0, 350.0, 100.0, 50.0));
        // Without a layout: the decoded vanilla MAINR.BIN geometry.
        let rects = menu_rects(None);
        assert_eq!(rects[0], Rect::new(400.0, 20.0, 179.0, 60.0));
        assert_eq!(rects[5], Rect::new(400.0, 400.0, 179.0, 60.0));
    }

    #[test]
    fn menu_labels_come_from_the_string_table() {
        // The importer lowercases keys; values are the shipped mixed-case text.
        let strings = HashMap::from([
            ("new_game".to_owned(), "New Game".to_owned()),
            ("quit".to_owned(), "Quit".to_owned()),
            // A localized table would substitute here.
            ("credits".to_owned(), "Mitwirkende".to_owned()),
            // An empty value must not blank the button.
            ("intro".to_owned(), String::new()),
        ]);
        let labels = menu_labels(Some(&strings));
        assert_eq!(labels.len(), MENU_ITEMS.len());
        assert_eq!(labels[0], "New Game");
        assert_eq!(labels[3], "Mitwirkende");
        assert_eq!(labels[5], "Quit");
        // Missing key and empty value both fall back to the shipped English.
        assert_eq!(labels[1], "Load Game");
        assert_eq!(labels[4], "Intro");
        // The repurposed Options slot reads what it does, whatever the string
        // table says (the override wins even over a shipped "Options").
        assert_eq!(labels[2], "Developer");
    }

    /// The body of the shipped `res/intrface/MAIN.STR`, verbatim (the file is
    /// CRLF-terminated, which the importer's line splitting strips). Kept here
    /// so a rename of a key - ours or the data's - fails a test rather than
    /// silently falling back to the English constants at runtime, which is
    /// invisible on an English install because the two agree.
    const SHIPPED_MAIN_STR: &str = concat!(
        "quit:\"Quit\"\n",
        "intro:\"Intro\"\n",
        "credits:\"Credits\"\n",
        "options:\"Options\"\n",
        "load_game:\"Load Game\"\n",
        "new_game:\"New Game\"\n",
    );

    #[test]
    fn every_item_key_resolves_against_the_shipped_string_table() {
        let lines: Vec<String> = SHIPPED_MAIN_STR.lines().map(|l| l.to_owned()).collect();
        let strings = dark::importers::parse_strings(&lines);

        // Every key this screen asks for must exist in the shipped table...
        for item in MENU_ITEMS {
            assert!(
                strings.contains_key(item.string_key),
                "MAIN.STR has no key '{}'",
                item.string_key
            );
        }
        // ...and the table must account for all six entries, so a seventh
        // shipped key would be a prompt to wire up another item.
        assert_eq!(strings.len(), MENU_ITEMS.len());

        // Resolved through the real parser, top to bottom - the Options slot
        // is overridden to read what it now does.
        assert_eq!(
            menu_labels(Some(&strings)),
            vec![
                "New Game",
                "Load Game",
                "Developer",
                "Credits",
                "Intro",
                "Quit"
            ]
        );
    }

    #[test]
    fn menu_labels_fall_back_without_a_string_table() {
        let labels = menu_labels(None);
        assert_eq!(
            labels,
            vec![
                "New Game",
                "Load Game",
                "Developer",
                "Credits",
                "Intro",
                "Quit"
            ]
        );
    }

    #[test]
    fn world_supports_transition_save_data() {
        // On "New Game", `switch_mission` calls `to_save_data` on the outgoing
        // scene's world, so the menu world must support it (PlayerInfo,
        // GlobalTemplateIdMap, ...). This would panic if a unique were missing.
        let scene = MainMenuScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
