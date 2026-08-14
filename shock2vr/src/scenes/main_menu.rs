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

use cgmath::{Quaternion, Rotation, Vector2, Vector3, vec2, vec3};
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
    GameOptions, PresentationMode,
    game_scene::GameScene,
    input_context::{InputContext, Pointer2D},
    mission::GlobalContext,
    scenes::frontend_sfx::FrontendSfx,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        HAlign, Rect, ScaleMode, UiCanvas, VAlign, VR_COMPONENT_Z_STEP, frontend_panel,
        pointer_to_canvas, ray_to_canvas,
    },
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
    LoadGame,
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
        action: Some(MenuAction::LoadGame),
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

/// A VR trigger past this counts as "pressed", matching the hand code's
/// grab/fire threshold.
const VR_TRIGGER_THRESHOLD: f32 = 0.5;

/// Where a hand is pointing on the menu panel, in canvas pixels, plus whether
/// its trigger is held.
///
/// Both controllers are always posed in VR, so "whichever hand hits the panel"
/// would always resolve to the same one. Instead a hand **with its trigger
/// held** wins, so either controller can click; ties and idle triggers fall
/// back to the right hand, which then drives the hover highlight.
fn vr_pointer(input_context: &InputContext) -> (Option<Vector2<f32>>, bool) {
    let panel = frontend_panel(input_context.head.rotation);
    let right = &input_context.right_hand;
    let left = &input_context.left_hand;
    let held = |hand: &crate::input_context::Hand| hand.trigger_value > VR_TRIGGER_THRESHOLD;

    let order = if held(left) && !held(right) {
        [left, right]
    } else {
        [right, left]
    };

    for hand in order {
        let direction = hand.rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        if let Some(point) =
            ray_to_canvas(vec2(CANVAS_W, CANVAS_H), &panel, hand.position, direction)
        {
            return (Some(point), held(hand));
        }
    }

    // Neither hand points at the menu; report the trigger anyway so a press
    // that starts off-panel is still consumed as "held" rather than becoming a
    // fresh edge the moment the ray crosses onto a button.
    (None, held(right) || held(left))
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
) -> (Option<MenuAction>, bool) {
    if !pressed || last_pressed {
        return (None, pressed);
    }
    (point.and_then(|p| hit(p, rects)), pressed)
}

/// The menu entry at a canvas point, if any. Shared by the click and the
/// rollover sound so the two can never disagree about where an entry is.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<MenuAction> {
    let mut canvas = UiCanvas::<MenuAction>::with_events(vec2(CANVAS_W, CANVAS_H));
    for (item, rect) in MENU_ITEMS.iter().zip(rects) {
        // Unimplemented entries get no hit region at all, so a click over one
        // falls through as "nothing was clicked".
        if let Some(action) = item.action {
            // The backdrop already contains the button art; this button is the
            // shared canvas hit region for its label.
            canvas.button(*rect, "", action);
        }
    }
    canvas.click_at(point)
}

/// Pure click resolution: on a rising press edge over an implemented item
/// (`rects` is parallel to [`MENU_ITEMS`]), return its action. Also returns the
/// new `last_pressed` to track for the next frame.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<MenuAction>, bool, Option<Vector2<f32>>) {
    match pointer {
        Some(p) => {
            let point = pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            );
            let (action, pressed) = resolve_click_at(point, p.pressed, last_pressed, rects);
            (action, pressed, point)
        }
        None => (None, false, None),
    }
}

pub struct MainMenuScene {
    world: World,
    scene_name: String,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Where the VR controller ray last met the panel, in canvas pixels. The
    /// VR counterpart of `pointer`, already in canvas space.
    vr_pointer_canvas: Option<Vector2<f32>>,
    /// Head facing from the latest update, so `render` hangs the panel exactly
    /// where `update` hit-tested it.
    head_rotation: Quaternion<f32>,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` can map the pointer into
    /// canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
    /// The frontend's hum, rollover and select sounds.
    sfx: FrontendSfx<MenuAction>,
}

impl MainMenuScene {
    pub fn new() -> Self {
        let world = super::ui_scene_world();

        Self {
            world,
            scene_name: "main_menu".to_owned(),
            pointer: None,
            vr_pointer_canvas: None,
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            // A press held across a scene swap must not read as a click
            // here: both screens sit on the same 640x480 canvas and their
            // widgets overlap (the load screen's "Done" center falls inside
            // the menu's "Quit" rect), so starting "already pressed" makes
            // the next rising edge require a real release first.
            last_pressed: true,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
            sfx: FrontendSfx::new(),
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
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, LABELS_FILE);
        let labels = menu_labels(strings.as_deref());
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

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));

        // The panel hangs off the head's facing, so this is needed for the
        // render regardless of which presentation drives the pointer.
        self.head_rotation = input_context.head.rotation;

        let (action, last_pressed, point) =
            if game_options.presentation_mode == PresentationMode::Vr {
                // VR has no 2D cursor: the pointer is where a controller ray meets
                // the menu panel, and the trigger is the button.
                let (point, pressed) = vr_pointer(input_context);
                self.vr_pointer_canvas = point;
                self.pointer = None;
                let (action, last_pressed) =
                    resolve_click_at(point, pressed, self.last_pressed, &rects);
                (action, last_pressed, point)
            } else {
                self.pointer = input_context.pointer;
                resolve_click(
                    input_context.pointer,
                    self.last_pressed,
                    self.last_screen_size,
                    &rects,
                )
            };
        self.last_pressed = last_pressed;

        // Hover and click feedback, from the same point that drives the
        // highlight - so a sound plays exactly when an entry lights up.
        self.sfx.hover(point.and_then(|p| hit(p, &rects)));
        if action.is_some() {
            self.sfx.click();
        }

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
        // In flat presentation the menu is drawn in screen space in
        // `render_per_eye` (which has the screen size); the 3D scene is empty.
        if options.presentation_mode != PresentationMode::Vr {
            return (Vec::new(), vec3(0.0, 0.0, 0.0), identity);
        }

        // In VR there is no screen to draw on, so the same canvas is presented
        // on a world-space panel in front of the player.
        let panel = frontend_panel(self.head_rotation);
        let canvas = self.build_canvas(asset_cache, self.vr_pointer_canvas);
        let objects = canvas.render_world_space(
            asset_cache,
            panel.transform(),
            self.vr_pointer_canvas.map(|p| vec2(p.x, p.y)),
            None,
            VR_COMPONENT_Z_STEP,
        );
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
        self.last_screen_size = screen_size;
        // In VR the menu lives on a world-space panel drawn by `render`; a
        // screen-space copy here would paste the whole canvas over both eyes
        // and hide it.
        if options.presentation_mode == PresentationMode::Vr {
            return Vec::new();
        }
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });
        let canvas = self.build_canvas(asset_cache, pointer_canvas);
        canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.sfx.pump(asset_cache, audio_context);
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    fn on_exit(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.sfx.stop(audio_context);
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
    use crate::ui::FRONTEND_PANEL_DISTANCE;
    use cgmath::Rotation3;

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

        let mut scene = MainMenuScene::new();
        let held = pointer_at(574.5 / CANVAS_W, 436.0 / CANVAS_H, true);

        // First frame after the swap: the press is held, not new.
        let (action, last, _) = resolve_click(held, scene.last_pressed, SCREEN, &rects);
        assert_eq!(action, None, "a carried-over press must not activate Quit");
        scene.last_pressed = last;

        // Releasing and pressing again is a real click.
        let (_, last, _) = resolve_click(
            pointer_at(574.5 / CANVAS_W, 436.0 / CANVAS_H, false),
            scene.last_pressed,
            SCREEN,
            &rects,
        );
        let (action, _, _) = resolve_click(held, last, SCREEN, &rects);
        assert_eq!(action, Some(MenuAction::Quit));
    }

    /// A hand pointing at a given canvas point on the VR panel, with the
    /// trigger at `trigger`. Built by inverting `ray_to_canvas`: place the hand
    /// at the panel-plane offset and aim straight down -Z.
    /// The head facing the tests aim against: the default camera orientation.
    fn test_head() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    /// A hand aimed at a given canvas point, derived from the panel's own basis
    /// rather than world axes - so the test stays honest whichever way the
    /// panel ends up facing.
    fn hand_aimed_at(point: Vector2<f32>, trigger: f32) -> crate::input_context::Hand {
        let panel = frontend_panel(test_head());
        let u = point.x / CANVAS_W - 0.5;
        let v = 0.5 - point.y / CANVAS_H;
        let right = panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0));
        let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
        let normal = panel.normal();
        let target = panel.center + right * (u * panel.size.x) + up * (v * panel.size.y);

        crate::input_context::Hand {
            // Stand back along the panel's normal and aim at the target.
            position: target - normal * FRONTEND_PANEL_DISTANCE,
            rotation: Quaternion::from_arc(vec3(0.0, 0.0, -1.0), normal, None),
            trigger_value: trigger,
            ..crate::input_context::Hand::default()
        }
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
        let (point, pressed) = vr_pointer(&vr_input(hand_aimed_at(target, 1.0)));
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
        let (point, pressed) = vr_pointer(&input);
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
        let (_, pressed) = vr_pointer(&vr_input(hand_aimed_at(rects[0].center(), 0.2)));
        assert!(!pressed, "a barely-touched trigger must not click");
    }

    #[test]
    fn aiming_away_from_the_vr_panel_yields_no_point() {
        // Rotated 180 degrees: pointing behind the player, away from the panel.
        // Both hands must aim away - in VR both are always posed, so leaving
        // one at its default would have it pointing straight at the panel.
        let panel = frontend_panel(test_head());
        let away = || crate::input_context::Hand {
            // Aim directly opposite the panel, from the panel's own centre.
            position: panel.center,
            rotation: Quaternion::from_arc(vec3(0.0, 0.0, -1.0), -panel.normal(), None),
            trigger_value: 1.0,
            ..crate::input_context::Hand::default()
        };
        let input = InputContext {
            right_hand: away(),
            left_hand: away(),
            ..InputContext::default()
        };
        let (point, pressed) = vr_pointer(&input);
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
    fn no_pointer_means_no_action() {
        let (action, last, _) = resolve_click(None, true, SCREEN, &menu_rects(None));
        assert_eq!(action, None);
        assert!(!last);
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
    fn click_over_an_unimplemented_item_does_nothing() {
        // Rect 2 is "Options", still unimplemented: canvas y 172..232.
        let rects = menu_rects(None);
        assert!(rects[2].contains(vec2(512.0, 202.0)));
        let (action, _, _) = resolve_click(pointer_at(0.8, 0.4208, true), false, SCREEN, &rects);
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
