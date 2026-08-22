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

#[cfg(test)]
use cgmath::{InnerSpace, Rotation, vec3};
use cgmath::{Matrix4, Quaternion, Vector2, Vector3, vec2};
#[cfg(test)]
use dark::map::MapRect;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;

use crate::{
    GameOptions,
    input_context::InputContext,
    ui::{
        FrontendCanvasPresenter, FrontendMenu, FrontendMenuItem, HAlign, Rect, ScaleMode, UiCanvas,
        VAlign, dev_params_panel, hit_menu_item,
    },
};

#[cfg(test)]
use crate::{
    input_context::Pointer2D,
    ui::{
        resolve_click_at as shell_resolve_click_at, resolve_flat_click, resolve_menu_labels,
        resolve_menu_rects, vr_frontend_pointer_pass,
    },
};
#[cfg(test)]
use std::collections::HashMap;

/// The screen is authored on the original 640x480 `SIM.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "SIM.PCX";
/// Backdrop for the Developer page: the archive frame
/// [`dev_params_panel`]'s row geometry is laid out against, shared with the
/// standalone Developer scene so the page is identical whichever way it is
/// reached. Still an opaque full-screen cover, so the flat presentation's
/// "the world is already gone" property holds on this page too.
const DEVELOPER_BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
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
/// Opacity for an implemented entry the pointer is not over. Kept well clear
/// of [`HOVER_OPACITY`]: on a menu where one of the two live entries abandons
/// the run, "which one am I about to press" has to read at a glance.
const IDLE_OPACITY: f32 = 0.45;
/// Opacity for the entry under the pointer.
const HOVER_OPACITY: f32 = 1.0;

// Fallback button geometry, used only when `SIMR.BIN` is missing: the decoded
// vanilla values - a column of five 179x76 buttons at x=400 on a 92px pitch.
const FALLBACK_BUTTON_X: f32 = 400.0;
const FALLBACK_BUTTON_TOP: f32 = 20.0;
const FALLBACK_BUTTON_W: f32 = 179.0;
const FALLBACK_BUTTON_H: f32 = 76.0;
const FALLBACK_BUTTON_PITCH: f32 = 92.0;

// The VR comfort dim lives in `crate::ui::world_dim`, shared with the
// cyber-interface use mode; see that module for the full design rationale.
// Flat presentation needs none of it: `SIM.PCX` is an opaque full-screen
// backdrop, so the world is already gone. (The mission's own `screen_fade`
// quad is not this either - it is a screen-space cover emitted from
// `render_per_eye`, which pastes over both eyes in VR and is suppressed while
// paused anyway.)
use crate::ui::world_dim::{UNTRACKED_HEAD, dim_distance, dim_pose, world_dim_layer};
#[cfg(test)]
use crate::ui::world_dim::{WORLD_DIM_EXTENT_RATIO, world_dim_min_distance, world_dim_strength};

/// What a click on the pause menu asks `Game` to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAction {
    /// Close the menu and let the simulation run again.
    Resume,
    /// Abandon the run and go back to the main menu.
    QuitToMainMenu,
}

/// Which page of the overlay is showing. The Developer page is a page of the
/// same overlay rather than a scene precisely because the overlay is not a
/// scene: swapping scenes would unload the mission the player is tuning over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseMenuPage {
    Root,
    Developer,
}

/// What a click on a root-page entry means: either something [`PauseAction`]
/// the owning `Game` must act on, or navigation the overlay handles itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseMenuEntry {
    Action(PauseAction),
    /// Switch to the Developer page.
    Developer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseMenuTarget {
    Root(PauseMenuEntry),
    Developer(dev_params_panel::DevParamsEvent),
}

// `SIM.PCX` (native 640x480) has a vertical stack of five buttons down the
// right side. This list is in screen order, top to bottom, so an item's index
// is also its rect index in `SIMR.BIN`.
const MENU_ITEMS: &[FrontendMenuItem<PauseMenuEntry>] = &[
    FrontendMenuItem {
        string_key: "continue",
        fallback_label: "Continue",
        action: Some(PauseMenuEntry::Action(PauseAction::Resume)),
        label_override: None,
    },
    FrontendMenuItem {
        string_key: "save_game",
        fallback_label: "Save Game",
        action: None,
        label_override: None,
    },
    FrontendMenuItem {
        string_key: "load_game",
        fallback_label: "Load Game",
        action: None,
        label_override: None,
    },
    // The Options slot hosts the Developer page while no real options screen
    // exists, matching the main menu's slot.
    FrontendMenuItem {
        string_key: "options",
        fallback_label: "Options",
        action: Some(PauseMenuEntry::Developer),
        label_override: Some("Developer"),
    },
    FrontendMenuItem {
        string_key: "quit",
        fallback_label: "Quit to \\nMain Menu",
        action: Some(PauseMenuEntry::Action(PauseAction::QuitToMainMenu)),
        label_override: None,
    },
];

const FALLBACK_RECTS: [Rect; 5] = [
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
];

/// Resolve each menu item's canvas rect from the `SIMR.BIN` layout (falling
/// back to the vanilla geometry if it's absent). Parallel to [`MENU_ITEMS`].
#[cfg(test)]
fn menu_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    resolve_menu_rects(layout, &FALLBACK_RECTS)
}

/// Resolve each menu item's label from `SIM.STR`, falling back to the shipped
/// English text when the string table is absent. Parallel to [`MENU_ITEMS`].
#[cfg(test)]
fn menu_labels(strings: Option<&HashMap<String, String>>) -> Vec<String> {
    resolve_menu_labels(strings, MENU_ITEMS)
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

/// The menu entry at a canvas point, if any. Both the click and the hover
/// highlight go through this, so the two can never disagree about where an
/// entry is - or about which entries are live at all.
fn hit(point: Vector2<f32>, rects: &[Rect]) -> Option<PauseMenuEntry> {
    hit_menu_item(point, MENU_ITEMS, rects, |_| true)
}

fn target_at(
    page: PauseMenuPage,
    panel_rects: dev_params_panel::PanelRects,
    rects: &[Rect],
    point: Vector2<f32>,
) -> Option<PauseMenuTarget> {
    match page {
        PauseMenuPage::Root => hit(point, rects).map(PauseMenuTarget::Root),
        PauseMenuPage::Developer => {
            dev_params_panel::hit(panel_rects, point).map(PauseMenuTarget::Developer)
        }
    }
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
#[cfg(test)]
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
) -> (Option<PauseMenuEntry>, bool) {
    shell_resolve_click_at(point, pressed, last_pressed, |point| hit(point, rects))
}

/// Pure click resolution for the flat pointer on the root page: on a rising
/// press edge over an implemented item, return its entry, plus the
/// `last_pressed` to carry. Kept as the tests' composition of the two pure
/// halves `update` uses.
#[cfg(test)]
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<PauseMenuEntry>, bool, Option<Vector2<f32>>) {
    resolve_flat_click(
        pointer,
        last_pressed,
        screen_size,
        vec2(CANVAS_W, CANVAS_H),
        SCALE_MODE,
        |point| hit(point, rects),
    )
}

/// The pause overlay. Closed by default; [`PauseMenu::open`] arms it.
pub struct PauseMenu {
    open: bool,
    /// Which page the overlay is showing; reset to the root on every open.
    page: PauseMenuPage,
    menu: FrontendMenu<PauseMenuTarget>,
    /// The head pose from the latest update. The panel is world-locked, but the
    /// comfort dim follows the gaze, so it needs the live pose rather than the
    /// placement (see [`world_dim_layer`]).
    head: (Vector3<f32>, Quaternion<f32>),
    /// Set when a click closed the menu, and cleared once that click is
    /// released. See [`PauseMenu::suspends_scene`].
    closed_under_a_held_press: bool,
    /// The Developer page's widget rects, re-resolved from `GAMELODR.BIN`
    /// each update (the render path takes `&self`, so it reads them here).
    panel_rects: dev_params_panel::PanelRects,
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
            page: PauseMenuPage::Root,
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
            head: (
                Vector3::new(0.0, 0.0, 0.0),
                Quaternion::new(0.0, 0.0, 0.0, 0.0),
            ),
            closed_under_a_held_press: false,
            panel_rects: dev_params_panel::PanelRects::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the active scene must stay frozen this frame.
    ///
    /// Wider than [`is_open`](Self::is_open) by one beat: the trigger that
    /// pressed "Continue" is still down on the frame the menu closes, and
    /// `VirtualHand` - which was never advanced while paused - would read it as
    /// a fresh rising edge and fire the held weapon the instant the world came
    /// back. The scene stays suspended until that press is released, which is
    /// the same rising-edge rule the menu applies to itself on the way in.
    pub fn suspends_scene(&self) -> bool {
        self.open || self.closed_under_a_held_press
    }

    /// Clear the post-close latch once nothing is pressed any more.
    pub fn poll_release(&mut self, input_context: &InputContext) {
        if !self.closed_under_a_held_press {
            return;
        }
        let hand_held = |hand: &crate::input_context::Hand| {
            hand.trigger_value > crate::ui::VR_TRIGGER_THRESHOLD
        };
        let pressed = hand_held(&input_context.right_hand)
            || hand_held(&input_context.left_hand)
            || input_context.pointer.is_some_and(|p| p.pressed);
        if !pressed {
            self.closed_under_a_held_press = false;
        }
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
        // Always land on the root page: reopening straight onto a parameter
        // list the player forgot they left would read as a broken menu.
        self.page = PauseMenuPage::Root;
        self.menu.reset_on_entry();
        // Forget the previous session's head: if `render` runs before the first
        // `update` of this one, a stale pose would hang the dim where the player
        // was standing last time. Untracked takes the panel fallback instead.
        self.head = UNTRACKED_HEAD;
    }

    /// Close because an entry was clicked - the press that did it is still
    /// held, so latch the scene shut until it is released.
    pub fn close_after_click(&mut self) {
        self.closed_under_a_held_press = true;
        self.close();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.menu.clear_pointer();
    }

    pub fn pump_sfx(
        &mut self,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        self.menu.pump_sfx(asset_cache, audio_context);
    }

    pub fn stop_sfx(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.menu.stop_sfx(audio_context);
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
        // The Developer page's rows ride the backdrop's authored widget rects,
        // so they follow the layout file rather than hardcoded numbers.
        self.panel_rects = dev_params_panel::rects(asset_cache);
        self.head = (input_context.head.position, input_context.head.rotation);

        let page = self.page;
        let panel_rects = self.panel_rects;
        let target = self.menu.update(
            elapsed,
            input_context,
            options.presentation_mode,
            |point| target_at(page, panel_rects, &rects, point),
            |point| target_at(page, panel_rects, &rects, point),
        );
        self.handle_target(target)
    }

    /// Route one frame's resolved pointer state to whichever page is showing,
    /// carrying the rising-edge flag across a page turn.
    ///
    /// Split out of [`Self::update`] so the press edge is testable without a
    /// presentation: the highest-risk interaction on this overlay is a *held*
    /// press surviving a page turn, because the Developer page's "Done" rect
    /// (527,405,95,62) overlaps the root's "Quit to Main Menu" rect - so a
    /// press that leaves the Developer page could otherwise land on Quit the
    /// very next frame. Whichever page consumes the click stores the pressed
    /// flag, so the next frame's rising edge is already spent.
    #[cfg(test)]
    fn consume_pointer(
        &mut self,
        point: Option<Vector2<f32>>,
        pressed: bool,
        rects: &[Rect],
    ) -> Option<PauseAction> {
        let page = self.page;
        let panel_rects = self.panel_rects;
        let target = self.menu.resolve_pointer(
            point,
            pressed,
            |point| target_at(page, panel_rects, rects, point),
            |point| target_at(page, panel_rects, rects, point),
        );
        self.handle_target(target)
    }

    fn handle_target(&mut self, target: Option<PauseMenuTarget>) -> Option<PauseAction> {
        match target {
            Some(PauseMenuTarget::Root(entry)) => self.handle_root_entry(Some(entry)),
            Some(PauseMenuTarget::Developer(event)) => self.handle_developer_event(Some(event)),
            None => None,
        }
    }

    /// Route a clicked root entry: actions go to the owning `Game`, the
    /// Developer entry switches pages inside the overlay.
    fn handle_root_entry(&mut self, entry: Option<PauseMenuEntry>) -> Option<PauseAction> {
        match entry {
            Some(PauseMenuEntry::Action(action)) => Some(action),
            Some(PauseMenuEntry::Developer) => {
                self.page = PauseMenuPage::Developer;
                None
            }
            None => None,
        }
    }

    /// Route a clicked Developer-page event: parameter steps mutate the
    /// registry, "Done" returns to the root page. Never a [`PauseAction`] -
    /// nothing on this page closes the menu or reaches `Game`.
    fn handle_developer_event(
        &mut self,
        event: Option<dev_params_panel::DevParamsEvent>,
    ) -> Option<PauseAction> {
        if let Some(event) = event {
            if dev_params_panel::activate(event) {
                self.page = PauseMenuPage::Root;
            }
        }
        None
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
        if !self.open {
            return Vec::new();
        }
        FrontendCanvasPresenter::new(options.presentation_mode, SCALE_MODE).present_world_space(
            || {
                let panel = self.menu.panel();
                let canvas = self.build_canvas(asset_cache, self.menu.pointer_canvas());
                // First in the list, and therefore first in the overlay group: the
                // comfort dim, which the depth clear below rides on.
                let (dim_position, dim_forward) = dim_pose(self.head.0, self.head.1, &panel);
                let mut objects = vec![world_dim_layer(
                    dim_position,
                    dim_forward,
                    dim_distance(dim_position, &panel),
                    crate::util::render_source::PAUSE_DIM,
                )];
                let canvas_objects =
                    self.menu
                        .render_world_space(asset_cache, canvas, options.presentation_mode);
                // The controllers and their aim rays, drawn from the same pass that
                // resolved the highlight, so the beams can never promise a hover the
                // menu will not give. The canvas objects already in hand are the layer
                // stack the hit dot has to float clear of - the dim is not one of them,
                // it hangs behind the panel rather than on it.
                objects.extend(canvas_objects);
                // Everything above was built in the tracked play space (where the head,
                // the hands and therefore the panel anchor live). A frontend *scene*
                // has no pawn, so it renders that space directly; the pause menu hangs
                // over a mission whose pawn is wherever the player is standing, so it
                // rebases every object into world coordinates here - once, at the
                // boundary, rather than by anchoring the panel differently.
                for object in &mut objects {
                    object.set_transform(pawn_to_world * object.get_transform());
                }
                // Game assigns this whole ordered stack to the system-overlay layer:
                // dim first, then panel and rays, all over the world and scene UI.
                objects
            },
        )
    }

    /// Screen-space presentation. Empty when closed or in VR (where a
    /// screen-space copy would paste the canvas over both eyes).
    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        if !self.open {
            return Vec::new();
        }
        FrontendCanvasPresenter::new(options.presentation_mode, SCALE_MODE).present_screen_space(
            || {
                let pointer_canvas = self.menu.screen_pointer_canvas(screen_size);
                let canvas = self.build_canvas(asset_cache, pointer_canvas);
                self.menu.render_screen_space(
                    asset_cache,
                    canvas,
                    screen_size,
                    options.presentation_mode,
                )
            },
        )
    }

    fn rects(&self, asset_cache: &mut AssetCache) -> Vec<Rect> {
        self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS)
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

        if self.page == PauseMenuPage::Developer {
            // The Developer page: the shared parameter panel on its own
            // backdrop. Everything about the page - rows, arrows, "Done" -
            // is described by `dev_params_panel`, so this page and the
            // standalone Developer scene cannot drift apart.
            canvas.image(
                Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H),
                DEVELOPER_BACKDROP_TEXTURE,
            );
            dev_params_panel::draw(&mut canvas, self.panel_rects, pointer_canvas);
            return canvas;
        }

        // The original's opaque full-screen backdrop. In flat presentation this
        // is also the "dim the world" answer: it covers the view exactly as the
        // original pause screen does. In VR it covers the panel only - but the
        // panel is large and close enough to fill most of the field of view, so
        // in practice the world reads as a border around it rather than a
        // backdrop. Sizing the VR panel deliberately (and dimming what is left
        // of the world behind it) is follow-up work this skeleton does not do.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let rects = self.rects(asset_cache);
        let labels = self.menu.labels(asset_cache, LABELS_FILE, MENU_ITEMS);
        // The highlight resolves through the very same `hit` the click does, so
        // an entry can never light up under a ray that would not activate it.
        let hovered = pointer_canvas.and_then(|p| hit(p, &rects));

        for ((item, rect), label) in MENU_ITEMS.iter().zip(&rects).zip(&labels) {
            let opacity = if item.action.is_none() {
                DISABLED_OPACITY
            } else if item.action == hovered {
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
    use cgmath::{Deg, Rad, Zero, vec4};
    use engine::scene::RenderLayer;

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
            assert_eq!(action, Some(PauseMenuEntry::Action(expected)));
            assert!(last);
        }
    }

    #[test]
    fn the_stubbed_entries_are_inert() {
        let rects = menu_rects(None);
        for index in [SAVE_INDEX, LOAD_INDEX] {
            let (action, _, _) =
                resolve_click(pointer_at(rects[index], true), false, SCREEN, &rects);
            assert_eq!(action, None, "entry {index} is a dimmed stub");
        }
    }

    #[test]
    fn clicking_the_developer_slot_switches_pages_without_reaching_game() {
        let rects = menu_rects(None);
        let (entry, _, _) = resolve_click(
            pointer_at(rects[OPTIONS_INDEX], true),
            false,
            SCREEN,
            &rects,
        );
        assert_eq!(entry, Some(PauseMenuEntry::Developer));

        let mut menu = PauseMenu::new();
        menu.open();
        assert_eq!(menu.page, PauseMenuPage::Root);
        // Routing the click: no PauseAction escapes to `Game` (the mission
        // must stay paused, the menu must stay open) - the overlay just
        // turns its page.
        assert_eq!(menu.handle_root_entry(entry), None);
        assert_eq!(menu.page, PauseMenuPage::Developer);
        assert!(menu.is_open());
    }

    #[test]
    fn done_on_the_developer_page_returns_to_the_root_page() {
        use crate::ui::dev_params_panel::DevParamsEvent;
        let mut menu = PauseMenu::new();
        menu.open();
        menu.handle_root_entry(Some(PauseMenuEntry::Developer));
        assert_eq!(menu.page, PauseMenuPage::Developer);

        // A frame with no click keeps the page.
        assert_eq!(menu.handle_developer_event(None), None);
        assert_eq!(menu.page, PauseMenuPage::Developer);

        // "Done" returns to the root - it does not close the menu, and it
        // does not reach `Game`. (The step events are not exercised here:
        // they mutate the process-global registry, which parallel tests
        // read - the SDK e2e proves a click really moves a value.)
        assert_eq!(
            menu.handle_developer_event(Some(DevParamsEvent::Done)),
            None
        );
        assert_eq!(menu.page, PauseMenuPage::Root);
        assert!(menu.is_open());
    }

    /// The overlay's most dangerous interaction: the Developer page's "Done"
    /// rect OVERLAPS the root page's "Quit to Main Menu" button, so a press
    /// that is still held when "Done" turns the page back would land on Quit
    /// the very next frame - abandoning the run from a menu that was only
    /// meant to close a settings page. This is the game-over insta-Quit bug
    /// class (vr-ui-design rule 6), so it is asserted through `consume_pointer`,
    /// which carries the real `last_pressed` flag, rather than through the
    /// page handlers that bypass it.
    #[test]
    fn a_press_held_through_done_cannot_fall_through_onto_quit() {
        let rects = menu_rects(None);
        let panel = dev_params_panel::PanelRects::default();
        let done = panel.done_center();
        // The premise: the two rects really do overlap, so this is a live
        // hazard and not a hypothetical one.
        assert!(
            rects[QUIT_INDEX].contains(done),
            "Done must sit over Quit for this test to mean anything"
        );

        let mut menu = PauseMenu::new();
        menu.open();
        menu.handle_root_entry(Some(PauseMenuEntry::Developer));
        assert_eq!(menu.page, PauseMenuPage::Developer);
        // Nothing pressed yet, so the next frame is a genuine rising edge.
        menu.menu.set_last_pressed(false);

        // Frame 1: press on "Done" - the page turns back to the root.
        assert_eq!(menu.consume_pointer(Some(done), true, &rects), None);
        assert_eq!(menu.page, PauseMenuPage::Root);

        // Frame 2: the SAME press is still held, over the Quit button now
        // under the pointer. It must not activate.
        assert_eq!(
            menu.consume_pointer(Some(done), true, &rects),
            None,
            "a held press must not fall through onto Quit after the page turn"
        );
        assert!(menu.is_open());

        // Only a real release and a fresh press reaches Quit.
        assert_eq!(menu.consume_pointer(Some(done), false, &rects), None);
        assert_eq!(
            menu.consume_pointer(Some(done), true, &rects),
            Some(PauseAction::QuitToMainMenu)
        );
    }

    /// The mirror image: the press that opens the Developer page must not
    /// immediately step a parameter with the same still-held press.
    #[test]
    fn a_press_held_into_the_developer_page_does_not_step_a_param() {
        let rects = menu_rects(None);
        let mut menu = PauseMenu::new();
        menu.open();
        menu.menu.set_last_pressed(false);

        let developer = rects[OPTIONS_INDEX].center();
        assert_eq!(menu.consume_pointer(Some(developer), true, &rects), None);
        assert_eq!(menu.page, PauseMenuPage::Developer);
        // Still held on the next frame: the edge is already spent, so no
        // panel event is resolved at all.
        assert!(menu.menu.last_pressed());
        assert_eq!(menu.consume_pointer(Some(developer), true, &rects), None);
        assert_eq!(menu.page, PauseMenuPage::Developer);
    }

    #[test]
    fn reopening_lands_on_the_root_page() {
        let mut menu = PauseMenu::new();
        menu.open();
        menu.handle_root_entry(Some(PauseMenuEntry::Developer));
        menu.close();
        menu.open();
        assert_eq!(
            menu.page,
            PauseMenuPage::Root,
            "a reopened menu must not resume on the parameter list"
        );
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
        // The repurposed Options slot reads what it does, whatever the
        // string table says.
        assert_eq!(labels[OPTIONS_INDEX], "Developer");
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
                "Developer",
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
                Some(PauseMenuEntry::Action(expected))
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
        let (action, last) = resolve_click_at(point, pressed, true, &rects);
        assert_eq!(action, None, "a carried-over press must not quit the run");

        // Releasing and pressing again is a real click.
        let (_, last) = resolve_click_at(point, false, last, &rects);
        assert_eq!(
            resolve_click_at(point, true, last, &rects).0,
            Some(PauseMenuEntry::Action(PauseAction::QuitToMainMenu))
        );
    }

    #[test]
    fn a_press_held_across_frames_clicks_once() {
        let rects = menu_rects(None);
        let point = Some(rects[RESUME_INDEX].center());
        let (action, last) = resolve_click_at(point, true, false, &rects);
        assert_eq!(action, Some(PauseMenuEntry::Action(PauseAction::Resume)));
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

    /// The desktop opening frame: mouse-look still has the cursor captured, so
    /// `InputContext::pointer` is `None`. Treating that as a release would
    /// discard `open()`'s guard and let a still-held button click on frame 2.
    #[test]
    fn a_frame_without_a_pointer_is_not_a_release() {
        let rects = menu_rects(None);
        let (action, last, _) = resolve_click(None, true, SCREEN, &rects);
        assert_eq!(action, None);
        assert!(last, "a missing pointer must not clear the press guard");
    }

    /// The trigger that pressed "Continue" is still down on the frame the menu
    /// closes; `VirtualHand` was never advanced while paused, so resuming into
    /// it reads as a fresh rising edge and fires the held weapon.
    #[test]
    fn the_scene_stays_suspended_until_the_selecting_press_is_released() {
        let mut menu = PauseMenu::new();
        menu.open();
        assert!(menu.suspends_scene());

        menu.close_after_click();
        assert!(!menu.is_open());
        assert!(menu.suspends_scene(), "the click is still held");

        // Still held: a trigger over the threshold keeps the world frozen.
        let mut held = InputContext::default();
        held.right_hand.trigger_value = 1.0;
        menu.poll_release(&held);
        assert!(menu.suspends_scene());

        // The flat pointer counts too.
        let clicking = InputContext {
            pointer: Some(Pointer2D {
                position: vec2(0.5, 0.5),
                pressed: true,
            }),
            ..InputContext::default()
        };
        menu.poll_release(&clicking);
        assert!(menu.suspends_scene());

        menu.poll_release(&InputContext::default());
        assert!(!menu.suspends_scene(), "released: the world may run again");
    }

    /// A toggle-close (the menu button, not a click) has nothing held to wait
    /// for, so it must not strand the simulation.
    #[test]
    fn closing_with_the_toggle_resumes_immediately() {
        let mut menu = PauseMenu::new();
        menu.open();
        menu.close();
        assert!(!menu.suspends_scene());
    }

    /// The highlight must come from the same hit test as the click, so a
    /// dimmed stub can never light up and a live entry can never fail to.
    #[test]
    fn only_the_entry_a_click_would_activate_is_highlighted() {
        let rects = menu_rects(None);
        assert_eq!(
            hit(rects[RESUME_INDEX].center(), &rects),
            Some(PauseMenuEntry::Action(PauseAction::Resume))
        );
        assert_eq!(hit(rects[SAVE_INDEX].center(), &rects), None);
        assert_eq!(
            hit(rects[OPTIONS_INDEX].center(), &rects),
            Some(PauseMenuEntry::Developer)
        );
        assert_eq!(
            hit(rects[QUIT_INDEX].center(), &rects),
            Some(PauseMenuEntry::Action(PauseAction::QuitToMainMenu))
        );
    }

    fn head_looking(yaw_deg: f32, pitch_deg: f32) -> Quaternion<f32> {
        use cgmath::Rotation3;
        Quaternion::from_angle_y(Deg(yaw_deg)) * Quaternion::from_angle_x(Deg(pitch_deg))
    }

    /// The dim is a *comfort* layer, so the thing to pin is that it actually
    /// darkens: an object that renders at full transparency is a no-op that
    /// would still pass a "the layer exists" test.
    #[test]
    fn the_dim_layer_darkens_the_world() {
        let panel = test_panel();
        let (position, forward) = dim_pose(Vector3::zero(), head_looking(0.0, 0.0), &panel);
        let object = world_dim_layer(
            position,
            forward,
            dim_distance(position, &panel),
            crate::util::render_source::PAUSE_DIM,
        );
        let transparency = object
            .effective_transparency()
            .expect("the dim must draw translucent, not opaque");
        assert!(
            (transparency - (1.0 - world_dim_strength())).abs() < 1e-6,
            "the tuning constant must reach the material: {transparency}"
        );
        assert!(
            (0.05..0.95).contains(&transparency),
            "fully opaque would hide the world, fully clear would not dim it: {transparency}"
        );
    }

    /// The device bug this replaced: a box drawn from inside needs the host to
    /// agree which winding faces the viewer, and the Quest did not - half of it
    /// was culled, so only parts of the view were dimmed. One double-sided quad
    /// rasterizes once per pixel on any host: it can neither vanish nor
    /// double-blend (which would silently square the authored strength).
    #[test]
    fn the_dim_layer_does_not_depend_on_backface_culling() {
        let panel = test_panel();
        let (position, forward) = dim_pose(Vector3::zero(), head_looking(0.0, 0.0), &panel);
        assert_eq!(
            world_dim_layer(
                position,
                forward,
                dim_distance(position, &panel),
                crate::util::render_source::PAUSE_DIM,
            )
            .backface_culling(),
            None,
            "the dim must not opt into culling - a host that disagrees about \
             winding would cull it away"
        );
    }

    /// Coverage is the whole point, and it must not depend on where the player
    /// is looking or on how wide the headset's field of view is. A viewer-locked
    /// quad at ratio `r` subtends `atan(r)` off-axis from the eye, in every
    /// direction, at every gaze angle.
    #[test]
    fn the_dim_layer_covers_the_view_at_any_gaze_angle() {
        let panel = test_panel();
        let eye = vec3(1.0, 1.6, -2.0);
        // A Quest's half-field is about 55 degrees; leave real margin over it.
        let half_field = Deg(55.0);

        for (yaw, pitch) in [
            (0.0, 0.0),
            (75.0, 0.0),
            (180.0, 0.0),
            (-120.0, 0.0),
            (0.0, 89.0),
            (0.0, -89.0),
            (37.0, -62.0),
        ] {
            let (position, forward) = dim_pose(eye, head_looking(yaw, pitch), &panel);
            assert!(
                (position - eye).magnitude() < 1e-5,
                "the dim must ride the eye, not the panel"
            );
            let object = world_dim_layer(
                position,
                forward,
                dim_distance(position, &panel),
                crate::util::render_source::PAUSE_DIM,
            );
            let center = object.get_transform().w.truncate();

            // Centred straight down the gaze...
            let to_center = (center - eye).normalize();
            assert!(
                (to_center - forward.normalize()).magnitude() < 1e-4,
                "the dim must sit on the gaze axis at ({yaw}, {pitch})"
            );
            // ...at a fixed distance, so the half-angle it subtends is fixed too.
            let distance = (center - eye).magnitude();
            assert!((distance - dim_distance(position, &panel)).abs() < 1e-4);

            // ...and square to the gaze, not merely centred on it: a quad
            // rotated about the gaze axis, or built facing away, would satisfy
            // every assertion above while presenting the view its edge.
            let transform = object.get_transform();
            for (x, y) in [(0.5, 0.5), (0.5, -0.5), (-0.5, 0.5), (-0.5, -0.5)] {
                let corner = (transform * vec4(x, y, 0.0, 1.0)).truncate();
                let depth = (corner - eye).dot(forward.normalize());
                assert!(
                    (depth - distance).abs() < 1e-3,
                    "corner ({x}, {y}) sits at depth {depth}, not {distance}: the \
                     dim is not perpendicular to the gaze at ({yaw}, {pitch})"
                );
            }
        }

        // Distance-independent, so this is about the constants, not the loop.
        let half_angle = Deg::from(Rad(WORLD_DIM_EXTENT_RATIO.atan())).0;
        assert!(
            half_angle > half_field.0 + 20.0,
            "coverage is only +/-{half_angle} degrees against a {half_field:?} half-field"
        );
    }

    /// Straight up and straight down are where a look-at basis degenerates: the
    /// gaze is parallel to world up, so the usual up-cross-forward is a zero
    /// vector and normalizing it yields NaN - which would put the dim's whole
    /// transform out of the frustum and leave the view undimmed. Unlike the
    /// panel (gravity-aligned, so it never points vertically) the dim follows
    /// the true gaze and can hit this exactly.
    #[test]
    fn a_vertical_gaze_still_produces_a_finite_dim() {
        let panel = test_panel();
        let eye = vec3(0.0, 1.6, 0.0);
        for pitch in [90.0, -90.0] {
            let (position, forward) = dim_pose(eye, head_looking(0.0, pitch), &panel);
            let transform = world_dim_layer(
                position,
                forward,
                dim_distance(position, &panel),
                crate::util::render_source::PAUSE_DIM,
            )
            .get_transform();
            assert!(
                transform.x.truncate().magnitude().is_finite()
                    && transform.w.truncate().magnitude().is_finite(),
                "a {pitch} degree gaze produced a non-finite transform: {transform:?}"
            );
            // ...and still centred on the gaze, not snapped to some default axis.
            let to_center = (transform.w.truncate() - eye).normalize();
            assert!(
                (to_center - forward.normalize()).magnitude() < 1e-4,
                "the dim left the gaze axis at {pitch} degrees"
            );
        }
    }

    /// Behind the panel from wherever the player is standing - including after
    /// they physically step back in a room-scale space, which the world-locked
    /// panel does not follow until its lazy recenter fires. A dim that ended up
    /// in front would grey out the menu it exists to make readable.
    #[test]
    fn the_dim_layer_stays_behind_the_panel_even_after_stepping_back() {
        let panel = test_panel();
        let eye = panel.center + panel.normal() * crate::ui::frontend_panel_distance();

        for step_back in [0.0, 0.5, 1.0, 2.5, 6.0] {
            // Backing away from the panel along its own normal is the worst case.
            let head = eye + panel.normal() * step_back;
            let distance = dim_distance(head, &panel);
            let farthest_corner = {
                let right = panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0)) * panel.size.x * 0.5;
                let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0)) * panel.size.y * 0.5;
                [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
                    .into_iter()
                    .map(|(x, y)| (panel.center + right * x + up * y - head).magnitude())
                    .fold(0.0_f32, f32::max)
            };
            assert!(
                distance > farthest_corner,
                "after stepping back {step_back} m the dim is at {distance} m, \
                 inside the panel's farthest corner at {farthest_corner} m"
            );
            assert!(distance >= world_dim_min_distance());
        }
    }

    /// Coverage is a ratio, so pushing the dim out to clear the panel must not
    /// shrink the angle it subtends.
    #[test]
    fn pushing_the_dim_out_does_not_cost_coverage() {
        let panel = test_panel();
        let eye = panel.center + panel.normal() * 5.0;
        let distance = dim_distance(eye, &panel);
        assert!(
            distance > world_dim_min_distance(),
            "this case should push out"
        );
        let object = world_dim_layer(
            eye,
            -panel.normal(),
            distance,
            crate::util::render_source::PAUSE_DIM,
        );
        let half_extent = object.get_transform().x.truncate().magnitude() * 0.5;
        assert!(
            ((half_extent / distance) - WORLD_DIM_EXTENT_RATIO).abs() < 1e-3,
            "the half-angle must be distance-independent"
        );
    }

    /// Reopening the menu must not hang the dim off the pose the *previous*
    /// pause ended with, on a frame where `render` beats the first `update`.
    #[test]
    fn reopening_forgets_the_previous_sessions_head() {
        let mut menu = PauseMenu::new();
        menu.head = (vec3(100.0, 100.0, 100.0), head_looking(123.0, 45.0));
        menu.open();
        assert_eq!(
            menu.head, UNTRACKED_HEAD,
            "a stale pose must not carry over"
        );
    }

    /// An untracked head is the ZERO quaternion, and `rotate_vector` returns the
    /// input unrotated for it - a dim hung along that would face world -Z
    /// wherever the player is actually looking.
    #[test]
    fn an_untracked_head_falls_back_to_the_panel() {
        let panel = test_panel();
        let (position, forward) = dim_pose(vec3(9.0, 9.0, 9.0), Quaternion::zero(), &panel);
        assert!(
            (forward - -panel.normal()).magnitude() < 1e-4,
            "an untracked pose must fall back to the panel's own facing"
        );
        // ...and to the viewer position the panel implies, not to the garbage
        // position that came with the untracked pose.
        let implied_head = panel.center + panel.normal() * crate::ui::frontend_panel_distance();
        assert!((position - implied_head).magnitude() < 1e-4);
    }

    /// The dim belongs to the explicit system-overlay group, so a wall in the
    /// player's face cannot swallow it while leaving the menu floating over a
    /// bright world.
    #[test]
    fn the_dim_layer_is_not_depth_tested_against_the_world() {
        let panel = test_panel();
        let (position, forward) = dim_pose(Vector3::zero(), head_looking(0.0, 0.0), &panel);
        let object = world_dim_layer(
            position,
            forward,
            dim_distance(position, &panel),
            crate::util::render_source::PAUSE_DIM,
        );
        assert_eq!(object.render_layer(), RenderLayer::SystemOverlay);
        assert!(
            !object.depth_write,
            "the dim must not write depth: the panel draws after it"
        );
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
