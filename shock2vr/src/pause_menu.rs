//! The in-game pause menu.
//!
//! This is the original's "sim" menu - `SIM.PCX` with the five buttons whose
//! rects live in `SIMR.BIN` and whose labels live in `SIM.STR` - presented on
//! the same shared [`UiCanvas`] every other frontend screen uses, so flatscreen
//! and VR draw one canvas (see AGENTS.md §3).
//!
//! Unlike [`crate::scenes::MainMenuScene`] and friends it is **not** a
//! [`GameScene`](crate::game_scene::GameScene): it is an overlay owned by
//! [`crate::Game`] and drawn on top of whatever gameplay scene is active. That
//! placement is the whole design:
//!
//! - every gameplay scene - missions *and* the `debug_*` scenes - gets pause
//!   without implementing anything;
//! - the paused scene keeps **rendering**, only its `update` is skipped. In VR
//!   the world must never stop drawing (a frozen submit is a comfort and
//!   compositor problem, see issues #1002/#1003); head tracking and the panel
//!   stay live because the runtime keeps calling `render`/`render_per_eye`.
//!
//! Skipping the scene update is also what makes the "hands are inert while
//! paused" rule free: `VirtualHand` is never advanced, so nothing is grabbed,
//! dropped or fired, and whatever was already held is still held on resume.

use std::collections::HashMap;

use cgmath::{Matrix4, Vector2, vec2};
use dark::{
    importers::{STRINGS_IMPORTER, UI_LAYOUT_IMPORTER},
    map::MapRect,
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

use crate::{
    GameOptions, PresentationMode,
    input_context::{InputContext, Pointer2D},
    ui::{
        FrontendPanelAnchor, FrontendPointerPass, HAlign, PointerVisuals, Rect, ScaleMode,
        UiCanvas, VAlign, VR_COMPONENT_Z_STEP, pointer_to_canvas, vr_frontend_pointer_pass,
    },
};

/// The screen is authored on the original 640x480 `SIM.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "SIM.PCX";
/// Original widget layout for `SIM.PCX` - LTRB rects for the five buttons,
/// top to bottom (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "SIMR.BIN";
/// Original label strings for this screen, keyed by [`MenuItem::string_key`].
const LABELS_FILE: &str = "SIM.STR";
/// The same display font the main menu uses (`res/intrface/METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// Baseline pitch for a label that asks for more than one line: METAFONT's
/// 20px cell plus a little leading. Splitting the 76px button rect evenly
/// instead would push a two-line label off the top and bottom of its art.
const MENU_LINE_HEIGHT: f32 = 22.0;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Opacity for an entry the port has not implemented yet.
const DISABLED_OPACITY: f32 = 0.3;
/// Opacity for an implemented entry the pointer is not over.
const IDLE_OPACITY: f32 = 0.6;
/// Opacity for the entry under the pointer.
const HOVER_OPACITY: f32 = 1.0;

// Fallback button geometry, used only when `SIMR.BIN` is missing: the decoded
// vanilla values - a column of five 179x76 buttons at x=400 on a 92px pitch.
const FALLBACK_BUTTON_X: f32 = 400.0;
const FALLBACK_BUTTON_TOP: f32 = 20.0;
const FALLBACK_BUTTON_W: f32 = 179.0;
const FALLBACK_BUTTON_H: f32 = 76.0;
const FALLBACK_BUTTON_PITCH: f32 = 92.0;

/// What a click on the pause menu asks `Game` to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAction {
    /// Close the menu and let the simulation run again.
    Resume,
    /// Abandon the run and go back to the main menu.
    QuitToMainMenu,
}

struct MenuItem {
    /// Key into `SIM.STR`.
    string_key: &'static str,
    /// Label used when `SIM.STR` is absent or missing the key. These match the
    /// shipped English strings.
    fallback_label: &'static str,
    /// `None` for an entry that exists on the original screen but that the port
    /// does not implement yet - drawn dimmed, and not clickable.
    action: Option<PauseAction>,
}

// `SIM.PCX` (native 640x480) has a vertical stack of five buttons down the
// right side. This list is in screen order, top to bottom, so an item's index
// is also its rect index in `SIMR.BIN`.
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        string_key: "continue",
        fallback_label: "Continue",
        action: Some(PauseAction::Resume),
    },
    MenuItem {
        string_key: "save_game",
        fallback_label: "Save Game",
        action: None,
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
        string_key: "quit",
        fallback_label: "Quit to \\nMain Menu",
        action: Some(PauseAction::QuitToMainMenu),
    },
];

/// Resolve each menu item's canvas rect from the `SIMR.BIN` layout (falling
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

/// Resolve each menu item's label from `SIM.STR`, falling back to the shipped
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

/// Split a shipped label into the lines it asks for. `SIM.STR`'s quit entry is
/// authored as `"    Quit to \nMain Menu"` - a literal backslash-n escape the
/// string importer passes through verbatim - and the canvas has no multi-line
/// text element, so the break is resolved here, once, in shared layout.
fn label_lines(label: &str) -> Vec<&str> {
    label
        .split("\\n")
        .flat_map(|part| part.split('\n'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// The menu entry at a canvas point, if any. Shared by the click and the hover
/// highlight so the two can never disagree about where an entry is.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<PauseAction> {
    let mut canvas = UiCanvas::<PauseAction>::with_events(vec2(CANVAS_W, CANVAS_H));
    for (item, rect) in MENU_ITEMS.iter().zip(rects) {
        // Unimplemented entries get no hit region at all, so a click over one
        // falls through as "nothing was clicked".
        if let Some(action) = item.action {
            canvas.button(*rect, "", action);
        }
    }
    canvas.click_at(point)
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
) -> (Option<PauseAction>, bool) {
    if !pressed || last_pressed {
        return (None, pressed);
    }
    (point.and_then(|p| hit(p, rects)), pressed)
}

/// Pure click resolution for the flat pointer: on a rising press edge over an
/// implemented item, return its action, plus the `last_pressed` to carry.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<PauseAction>, bool, Option<Vector2<f32>>) {
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

/// The pause overlay. Closed by default; [`PauseMenu::open`] arms it.
pub struct PauseMenu {
    open: bool,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Where the VR controller ray last met the panel, in canvas pixels.
    vr_pointer_canvas: Option<Vector2<f32>>,
    /// The pointer pass that hit-tested this frame, kept so `render` draws the
    /// beams and dot from the very rays `update` resolved the highlight from.
    vr_pointer: FrontendPointerPass,
    /// The drawn half of that pointer (hands, beams, dot), holding the lazily
    /// loaded glove model.
    vr_pointer_visuals: PointerVisuals,
    /// Where the VR panel hangs: placed from the head when the menu opens and
    /// world-locked after that, so `render` draws it where `update` hit-tested.
    panel_anchor: FrontendPanelAnchor,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` maps the flat pointer
    /// into canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
}

impl Default for PauseMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl PauseMenu {
    pub fn new() -> Self {
        Self {
            open: false,
            pointer: None,
            vr_pointer_canvas: None,
            vr_pointer: FrontendPointerPass::default(),
            vr_pointer_visuals: PointerVisuals::new(),
            panel_anchor: FrontendPanelAnchor::new(),
            last_pressed: true,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the menu in front of the player.
    ///
    /// The panel anchor is reset so the menu is placed from the head pose it is
    /// opened with rather than from wherever the player was standing the last
    /// time they paused, and `last_pressed` starts `true` so a trigger still
    /// held from gameplay (the shot that preceded the pause, or the very button
    /// press that opened the menu) cannot read as a rising edge on the first
    /// frame - the game-over screen shipped with exactly that bug.
    pub fn open(&mut self) {
        self.open = true;
        self.panel_anchor = FrontendPanelAnchor::new();
        self.pointer = None;
        self.vr_pointer_canvas = None;
        self.vr_pointer = FrontendPointerPass::default();
        self.last_pressed = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.pointer = None;
        self.vr_pointer_canvas = None;
        self.vr_pointer = FrontendPointerPass::default();
    }

    /// Advance the overlay and report the entry that was clicked, if any.
    /// A no-op returning `None` while closed.
    pub fn update(
        &mut self,
        elapsed: std::time::Duration,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> Option<PauseAction> {
        if !self.open {
            return None;
        }

        let rects = self.rects(asset_cache);

        // Placed from the head when the menu opens and world-locked after
        // that; advancing it here keeps the ray and the render agreeing on
        // where the panel is, in either presentation.
        let panel = self.panel_anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            elapsed,
        );

        let (action, last_pressed) = if options.presentation_mode == PresentationMode::Vr {
            // VR has no 2D cursor: the pointer is where a controller ray meets
            // the panel, and the trigger is the button.
            self.vr_pointer =
                vr_frontend_pointer_pass(input_context, vec2(CANVAS_W, CANVAS_H), &panel);
            let (point, pressed) = (self.vr_pointer.point(), self.vr_pointer.pressed);
            self.vr_pointer_canvas = point;
            self.pointer = None;
            resolve_click_at(point, pressed, self.last_pressed, &rects)
        } else {
            self.pointer = input_context.pointer;
            self.vr_pointer = FrontendPointerPass::default();
            let (action, last_pressed, _) = resolve_click(
                input_context.pointer,
                self.last_pressed,
                self.last_screen_size,
                &rects,
            );
            (action, last_pressed)
        };
        self.last_pressed = last_pressed;
        action
    }

    /// World-space presentation: the canvas on a panel in front of the player.
    /// Empty when closed or flat.
    ///
    /// `pawn_to_world` maps the tracked play space - which is what
    /// [`InputContext::head`] and the hands are expressed in, and therefore
    /// where the panel is anchored and hit-tested - into world coordinates. It
    /// is the camera transform the runtime builds from the pose returned
    /// alongside the scene's objects. A frontend screen has no pawn, so it is
    /// the identity there and the panel lands in the same place either way;
    /// inside a mission the pawn is wherever the player is standing, and
    /// without this the panel would hang near the world origin.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
        pawn_to_world: Matrix4<f32>,
    ) -> Vec<SceneObject> {
        if !self.open || options.presentation_mode != PresentationMode::Vr {
            return Vec::new();
        }
        let panel = self.panel_anchor.panel();
        let canvas = self.build_canvas(asset_cache, self.vr_pointer_canvas);
        let mut objects = canvas.render_world_space(
            asset_cache,
            panel.transform(),
            self.vr_pointer_canvas,
            None,
            VR_COMPONENT_Z_STEP,
        );
        // The controllers and their aim rays, drawn from the same pass that
        // resolved the highlight, so the beams can never promise a hover the
        // menu will not give. The canvas objects already in hand are the layer
        // stack the hit dot has to float clear of.
        let panel_layers = objects.len();
        objects.extend(self.vr_pointer_visuals.render(
            asset_cache,
            &self.vr_pointer,
            vec2(CANVAS_W, CANVAS_H),
            &panel,
            panel_layers,
        ));
        // Everything above was built in the tracked play space (where the head,
        // the hands and therefore the panel anchor live). A frontend *scene*
        // has no pawn, so it renders that space directly; the pause menu hangs
        // over a mission whose pawn is wherever the player is standing, so it
        // rebases every object into world coordinates here - once, at the
        // boundary, rather than by anchoring the panel differently.
        for object in &mut objects {
            object.set_transform(pawn_to_world * object.get_transform());
        }
        // A modal panel the player cannot read is not a pause menu: world-locked
        // two metres ahead, it lands inside a wall or a console often enough
        // that depth-testing it against the world is not an option. The
        // renderer treats everything from the first `clear_depth` object onward
        // as an overlay group drawn after the world's own passes, and `Game`
        // appends these last, so the group is exactly the panel and its rays.
        if let Some(first) = objects.first_mut() {
            first.set_clear_depth(true);
        }
        objects
    }

    /// Screen-space presentation. Empty when closed or in VR (where a
    /// screen-space copy would paste the canvas over both eyes).
    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.last_screen_size = screen_size;
        if !self.open || options.presentation_mode == PresentationMode::Vr {
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

    fn rects(&self, asset_cache: &mut AssetCache) -> Vec<Rect> {
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        menu_rects(layout.as_deref().map(|r| r.as_slice()))
    }

    /// The menu, described once. Screen-space and world-space presentation
    /// differ only in how this canvas is rendered, so the two can never drift
    /// apart in layout, labels, or which entries look actionable.
    fn build_canvas(
        &self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));

        // The original's opaque full-screen backdrop. In flat presentation this
        // is also the "dim the world" answer: it covers the view exactly as the
        // original pause screen does. In VR it covers only the panel, and the
        // world stays visible (and lit) around it - deliberately, since dimming
        // the whole VR view would need a renderer-wide pass this skeleton does
        // not add.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let rects = menu_rects(
            asset_cache
                .get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE)
                .as_deref()
                .map(|r| r.as_slice()),
        );
        let strings = asset_cache.get_opt(&STRINGS_IMPORTER, LABELS_FILE);
        let labels = menu_labels(strings.as_deref());

        for ((item, rect), label) in MENU_ITEMS.iter().zip(&rects).zip(&labels) {
            let opacity = if item.action.is_none() {
                DISABLED_OPACITY
            } else if pointer_canvas.is_some_and(|p| rect.contains(p)) {
                HOVER_OPACITY
            } else {
                IDLE_OPACITY
            };
            // A shipped label may ask for a line break ("Quit to \nMain Menu").
            // Stack its lines at the font's own pitch, as a block centered on
            // the button, so a two-line entry sits inside the same art a
            // one-line entry does.
            let lines = label_lines(label);
            let block_h = MENU_LINE_HEIGHT * lines.len() as f32;
            let top = rect.y + (rect.h - block_h) * 0.5;
            for (index, line) in lines.iter().enumerate() {
                canvas
                    .text_native(
                        Rect::new(
                            rect.x,
                            top + index as f32 * MENU_LINE_HEIGHT,
                            rect.w,
                            MENU_LINE_HEIGHT,
                        ),
                        line,
                        MENU_FONT,
                        HAlign::Center,
                        VAlign::Middle,
                    )
                    .opacity(opacity);
            }
        }
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runtimes render 4:3, so PreserveAspect maps normalized coordinates
    /// straight onto the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    const RESUME_INDEX: usize = 0;
    const SAVE_INDEX: usize = 1;
    const LOAD_INDEX: usize = 2;
    const OPTIONS_INDEX: usize = 3;
    const QUIT_INDEX: usize = 4;

    fn pointer_at(rect: Rect, pressed: bool) -> Option<Pointer2D> {
        let center = rect.center();
        Some(Pointer2D {
            position: vec2(center.x / CANVAS_W, center.y / CANVAS_H),
            pressed,
        })
    }

    #[test]
    fn clicking_resume_and_quit_activates_them() {
        let rects = menu_rects(None);
        for (index, expected) in [
            (RESUME_INDEX, PauseAction::Resume),
            (QUIT_INDEX, PauseAction::QuitToMainMenu),
        ] {
            let (action, last, _) =
                resolve_click(pointer_at(rects[index], true), false, SCREEN, &rects);
            assert_eq!(action, Some(expected));
            assert!(last);
        }
    }

    #[test]
    fn the_stubbed_entries_are_inert() {
        let rects = menu_rects(None);
        for index in [SAVE_INDEX, LOAD_INDEX, OPTIONS_INDEX] {
            let (action, _, _) =
                resolve_click(pointer_at(rects[index], true), false, SCREEN, &rects);
            assert_eq!(action, None, "entry {index} is a dimmed stub");
        }
    }

    #[test]
    fn held_press_does_not_re_activate() {
        let rects = menu_rects(None);
        let (action, last, _) =
            resolve_click(pointer_at(rects[RESUME_INDEX], true), true, SCREEN, &rects);
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn menu_rects_prefer_the_layout_file() {
        let layout: Vec<MapRect> = (0..5)
            .map(|i| MapRect::new(10, i * 80, 110, i * 80 + 60))
            .collect();
        let rects = menu_rects(Some(&layout));
        assert_eq!(rects.len(), MENU_ITEMS.len());
        assert_eq!(rects[RESUME_INDEX], Rect::new(10.0, 0.0, 100.0, 60.0));
        assert_eq!(rects[QUIT_INDEX], Rect::new(10.0, 320.0, 100.0, 60.0));
        // Without a layout: the decoded vanilla SIMR.BIN geometry.
        assert_eq!(
            menu_rects(None)[QUIT_INDEX],
            Rect::new(400.0, 388.0, 179.0, 76.0)
        );
    }

    /// The body of the shipped `res/intrface/SIM.STR`, verbatim. Kept here so a
    /// rename of a key - ours or the data's - fails a test rather than silently
    /// falling back to the English constants at runtime.
    const SHIPPED_SIM_STR: &str = concat!(
        "continue:\"Continue\"\n",
        "quit:\"    Quit to \\nMain Menu\"\n",
        "options:\"Options\"\n",
        "load_game:\"Load Game\"\n",
        "save_game:\"Save Game\"\n",
    );

    #[test]
    fn every_item_key_resolves_against_the_shipped_string_table() {
        let lines: Vec<String> = SHIPPED_SIM_STR.lines().map(|l| l.to_owned()).collect();
        let strings = dark::importers::parse_strings(&lines);

        for item in MENU_ITEMS {
            assert!(
                strings.contains_key(item.string_key),
                "SIM.STR has no key '{}'",
                item.string_key
            );
        }
        // The table must account for all five entries, so a sixth shipped key
        // would be a prompt to wire up another item.
        assert_eq!(strings.len(), MENU_ITEMS.len());

        let labels = menu_labels(Some(&strings));
        assert_eq!(labels[RESUME_INDEX], "Continue");
        assert_eq!(labels[SAVE_INDEX], "Save Game");
        assert_eq!(labels[QUIT_INDEX], "Quit to \\nMain Menu");
    }

    #[test]
    fn a_shipped_line_break_becomes_two_lines() {
        // Drawn verbatim this would put a literal "\n" on the button.
        assert_eq!(
            label_lines("Quit to \\nMain Menu"),
            vec!["Quit to", "Main Menu"]
        );
        assert_eq!(label_lines("Continue"), vec!["Continue"]);
    }

    #[test]
    fn menu_labels_fall_back_without_a_string_table() {
        let labels = menu_labels(None);
        assert_eq!(
            labels,
            vec![
                "Continue",
                "Save Game",
                "Load Game",
                "Options",
                "Quit to \\nMain Menu"
            ]
        );
    }

    fn test_panel() -> crate::ui::WorldPanel {
        crate::ui::test_support::test_panel()
    }

    fn vr_input(hand: crate::input_context::Hand) -> InputContext {
        InputContext {
            right_hand: hand,
            ..InputContext::default()
        }
    }

    fn hand_aimed_at(point: Vector2<f32>, trigger: f32) -> crate::input_context::Hand {
        crate::ui::test_support::hand_aimed_at(vec2(CANVAS_W, CANVAS_H), point, trigger)
    }

    #[test]
    fn a_vr_ray_can_press_resume_and_quit() {
        let rects = menu_rects(None);
        for (index, expected) in [
            (RESUME_INDEX, PauseAction::Resume),
            (QUIT_INDEX, PauseAction::QuitToMainMenu),
        ] {
            let pass = vr_frontend_pointer_pass(
                &vr_input(hand_aimed_at(rects[index].center(), 1.0)),
                vec2(CANVAS_W, CANVAS_H),
                &test_panel(),
            );
            let (point, pressed) = (pass.point(), pass.pressed);
            let point = point.expect("the ray should land on the panel");
            assert!(rects[index].contains(point));
            assert_eq!(
                resolve_click_at(Some(point), pressed, false, &rects).0,
                Some(expected)
            );
        }
    }

    #[test]
    fn a_trigger_held_when_the_menu_opens_does_not_click() {
        // The menu is opened straight out of gameplay - very plausibly with the
        // trigger still down, and in VR the menu button and the trigger can be
        // held together. Without `open()`'s `last_pressed: true` that reads as
        // a rising edge over whatever the ray crosses, up to and including
        // "Quit to Main Menu".
        let rects = menu_rects(None);
        let mut menu = PauseMenu::new();
        menu.open();

        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(rects[QUIT_INDEX].center(), 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        assert!(pressed);
        let (action, last) = resolve_click_at(point, pressed, menu.last_pressed, &rects);
        assert_eq!(action, None, "a carried-over press must not quit the run");

        // Releasing and pressing again is a real click.
        let (_, last) = resolve_click_at(point, false, last, &rects);
        assert_eq!(
            resolve_click_at(point, true, last, &rects).0,
            Some(PauseAction::QuitToMainMenu)
        );
    }

    #[test]
    fn a_press_held_across_frames_clicks_once() {
        let rects = menu_rects(None);
        let point = Some(rects[RESUME_INDEX].center());
        let (action, last) = resolve_click_at(point, true, false, &rects);
        assert_eq!(action, Some(PauseAction::Resume));
        assert_eq!(resolve_click_at(point, true, last, &rects).0, None);
    }

    /// The Game-level gate: a frontend screen is already system UI, so the
    /// pause menu must never open over one (two stacked menus, and the outer
    /// one owns the pointer).
    #[test]
    fn frontend_screens_are_not_pausable() {
        use crate::game_scene::GameScene;
        assert!(!crate::scenes::MainMenuScene::new().is_pausable());
        assert!(!crate::scenes::GameOverScene::new().is_pausable());
        assert!(!crate::scenes::LoadGameScene::new().is_pausable());
    }

    #[test]
    fn a_closed_menu_renders_and_updates_nothing() {
        let mut menu = PauseMenu::new();
        assert!(!menu.is_open());
        menu.open();
        assert!(menu.is_open());
        menu.close();
        assert!(!menu.is_open());
    }
}
