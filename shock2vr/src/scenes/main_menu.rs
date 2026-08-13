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

use std::collections::HashMap;

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

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::{InputContext, Pointer2D},
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
};

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
    Quit,
}

struct MenuItem {
    /// Key into `MAIN.STR`.
    string_key: &'static str,
    /// Label used when `MAIN.STR` is absent or missing the key. These match the
    /// shipped English strings.
    fallback_label: &'static str,
    /// `None` for an entry that exists on the original screen but that the port
    /// does not implement yet - drawn dimmed, and not clickable.
    action: Option<MenuAction>,
}

// MAIN.PCX (native 640x480) has a vertical stack of six buttons down the right
// side. This list is in screen order, top to bottom, so an item's index is also
// its rect index in `MAINR.BIN`. Labels are centered in the button rect.
//
// (`MAIN.STR` itself lists the keys in reverse screen order; the same reversal
// holds for `SIM.STR` against the known pause-menu order.)
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        string_key: "new_game",
        fallback_label: "New Game",
        action: Some(MenuAction::NewGame),
    },
    MenuItem {
        string_key: "load_game",
        fallback_label: "Load Game",
        action: None,
    },
    MenuItem {
        string_key: "options",
        fallback_label: "Options",
        action: None,
    },
    MenuItem {
        string_key: "credits",
        fallback_label: "Credits",
        action: None,
    },
    MenuItem {
        string_key: "intro",
        fallback_label: "Intro",
        action: None,
    },
    MenuItem {
        string_key: "quit",
        fallback_label: "Quit",
        action: Some(MenuAction::Quit),
    },
];

/// Resolve each menu item's canvas rect from the `MAINR.BIN` layout (falling
/// back to the vanilla geometry if it's absent). Parallel to [`MENU_ITEMS`].
fn menu_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    MENU_ITEMS
        .iter()
        .enumerate()
        .map(
            |(index, _)| match layout.and_then(|rects| rects.get(index)) {
                Some(r) => Rect::new(
                    r.ul_x as f32,
                    r.ul_y as f32,
                    r.width() as f32,
                    r.height() as f32,
                ),
                None => Rect::new(
                    FALLBACK_BUTTON_X,
                    FALLBACK_BUTTON_TOP + index as f32 * FALLBACK_BUTTON_PITCH,
                    FALLBACK_BUTTON_W,
                    FALLBACK_BUTTON_H,
                ),
            },
        )
        .collect()
}

/// Resolve each menu item's label from `MAIN.STR`, falling back to the shipped
/// English text when the string table is absent. Parallel to [`MENU_ITEMS`].
fn menu_labels(strings: Option<&HashMap<String, String>>) -> Vec<String> {
    MENU_ITEMS
        .iter()
        .map(|item| {
            strings
                // The strings importer lowercases its keys.
                .and_then(|s| s.get(item.string_key))
                .filter(|label| !label.is_empty())
                .cloned()
                .unwrap_or_else(|| item.fallback_label.to_owned())
        })
        .collect()
}

/// Pure click resolution: on a rising press edge over an implemented item
/// (`rects` is parallel to [`MENU_ITEMS`]), return its action. Also returns the
/// new `last_pressed` to track for the next frame.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<MenuAction>, bool) {
    match pointer {
        Some(p) => {
            let action = if p.pressed && !last_pressed {
                let mut canvas = UiCanvas::<MenuAction>::with_events(vec2(CANVAS_W, CANVAS_H));
                for (item, rect) in MENU_ITEMS.iter().zip(rects) {
                    // Unimplemented entries get no hit region at all, so a
                    // click over one falls through as "nothing was clicked".
                    if let Some(action) = item.action {
                        // The backdrop already contains the button art; this
                        // button is the shared canvas hit region for its label.
                        canvas.button(*rect, "", action);
                    }
                }
                pointer_to_canvas(
                    vec2(CANVAS_W, CANVAS_H),
                    p.position,
                    screen_size,
                    SCALE_MODE,
                )
                .and_then(|point| canvas.click_at(point))
            } else {
                None
            };
            (action, p.pressed)
        }
        None => (None, false),
    }
}

pub struct MainMenuScene {
    world: World,
    scene_name: String,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` can map the pointer into
    /// canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
}

impl MainMenuScene {
    pub fn new() -> Self {
        let world = super::ui_scene_world();

        Self {
            world,
            scene_name: "main_menu".to_owned(),
            pointer: None,
            last_pressed: false,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
        }
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
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));

        self.pointer = input_context.pointer;
        let (action, last_pressed) = resolve_click(
            input_context.pointer,
            self.last_pressed,
            self.last_screen_size,
            &rects,
        );
        self.last_pressed = last_pressed;

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
            Some(MenuAction::Quit) => vec![Effect::GlobalEffect(GlobalEffect::Quit)],
            None => Vec::new(),
        }
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // The menu is drawn in screen space in `render_per_eye` (which has the
        // screen size); the 3D scene is empty.
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

        // Full-screen backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), "MAIN.PCX");

        // Menu items, centered in their button and brighter when hovered.
        // Button rects come from the original `MAINR.BIN` layout and labels
        // from `MAIN.STR` (both cached by the asset cache after first load).
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, LABELS_FILE);
        let labels = menu_labels(strings.as_deref());
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });
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
        let (action, last) = resolve_click(
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
        let (action, _) = resolve_click(
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
        let (action, last) = resolve_click(
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
        let (action, _) = resolve_click(
            pointer_at(0.05, 0.05, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, None);
    }

    #[test]
    fn no_pointer_means_no_action() {
        let (action, last) = resolve_click(None, true, SCREEN, &menu_rects(None));
        assert_eq!(action, None);
        assert!(!last);
    }

    #[test]
    fn click_over_an_unimplemented_item_does_nothing() {
        // Rect 1 is "Load Game", which has no action yet: canvas y 96..156, so
        // normalized y ~0.26 sits inside it.
        let rects = menu_rects(None);
        assert!(rects[1].contains(vec2(512.0, 126.0)));
        let (action, _) = resolve_click(pointer_at(0.8, 0.2625, true), false, SCREEN, &rects);
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

        // Resolved through the real parser, top to bottom.
        assert_eq!(
            menu_labels(Some(&strings)),
            vec![
                "New Game",
                "Load Game",
                "Options",
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
                "Options",
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
