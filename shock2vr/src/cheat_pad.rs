//! The developer cheat pad.
//!
//! A two-button panel that rains useful items around the player, for setting
//! up a test situation without playing the game up to it. Like
//! [`crate::pause_menu`] it is an overlay owned by [`crate::Game`] rather than
//! a [`GameScene`](crate::game_scene::GameScene), so it draws over any pausable
//! scene without that scene implementing anything, the world keeps rendering
//! behind it (a frozen submit is a VR comfort problem), and only the scene's
//! `update` is skipped while it is up. The *rain* still needs a scene that
//! handles [`Effect::RainItems`] - missions and the `debug_*` scenes built on
//! `MissionCore` do; a scene using the trait's default effect handler (e.g.
//! `debug_hud`) shows the pad but drops the spawn.
//!
//! It is gated on the `cheats` developer parameter: with that off the open
//! action is never even read, so neither a stray controller chord nor an HTTP
//! injection can conjure free weapons into an ordinary run.
//!
//! The page rides [`dev_params_panel`]'s authored widget rects (`GAMELODR.BIN`
//! over `GAMELOD.PCX`), so its placement is resolved once, in canvas pixels,
//! and flatscreen and VR draw the identical canvas (AGENTS.md section 3).

use cgmath::{Matrix4, Quaternion, Vector2, Vector3, vec2};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext, scene::SceneObject};
use shipyard::EntityId;

use crate::ui::world_dim::{UNTRACKED_HEAD, dim_distance, dim_pose, world_dim_layer};
use crate::{
    GameOptions,
    input_context::InputContext,
    scripts::Effect,
    ui::{
        FrontendCanvasPresenter, FrontendMenu, HAlign, Rect, ScaleMode, UiCanvas, VAlign,
        dev_params_panel::{self, PanelRects},
    },
};

/// The page is authored on the original 640x480 canvas, like every other
/// frontend screen.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// The archive frame the Developer page uses, shared so the two developer
/// pages read as one screen family.
const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
const MENU_FONT: &str = "metafont.fon";
/// The smaller face the Developer page's rows use. The display font
/// ellipsizes "Rain modules + nanites" inside the 202px pane.
const ROW_FONT: &str = "mainfont.fon";
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Opacity for a button the pointer is not over, and for the one it is.
/// Matches [`dev_params_panel`]'s pair so the two pages highlight alike.
const IDLE_OPACITY: f32 = 0.65;
const HOVER_OPACITY: f32 = 1.0;

/// Button geometry inside the panel's list pane: a horizontal inset matching
/// the parameter rows', and rows tall enough to be an easy VR ray target.
const BUTTON_INSET: f32 = 8.0;
const BUTTON_H: f32 = 40.0;
const BUTTON_PITCH: f32 = 56.0;

/// Templates rained by [`CheatButton::RainWeapons`]: the four workhorse
/// weapons plus a clip for each gun that takes one. Resolved from the gamesys
/// (`cargo dq entities medsci1.mis --filter "*Clip*"`).
const WEAPON_TEMPLATES: &[i32] = &[
    -928,  // Wrench
    -17,   // Pistol
    -19,   // Shotgun
    -18,   // Assault Rifle
    -1358, // Small Standard Clip
    -1358, // Small Standard Clip
    -1360, // Small AP Clip
    -42,   // Pellet Shot Box (shotgun shells)
];

/// Templates rained by [`CheatButton::RainModules`]: cyber modules and
/// nanites, the two things a test situation is usually short of.
const MODULE_TEMPLATES: &[i32] = &[
    -938, // EXP Cookies (cyber modules)
    -938, -938, -938, //
    -89,  // 20 Nanites
    -89, -89, -89,
];

/// What a click on the pad does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatButton {
    RainWeapons,
    RainModules,
    /// Close the pad and let the simulation run again.
    Done,
}

impl CheatButton {
    /// The two rain buttons, top to bottom. `Done` is not here: it rides the
    /// backdrop's own "Done" rect rather than a list row.
    const ROWS: [(CheatButton, &'static str); 2] = [
        (CheatButton::RainWeapons, "Rain weapons"),
        (CheatButton::RainModules, "Rain modules + nanites"),
    ];

    /// The effect this button asks the scene for; `None` for `Done`, which
    /// only closes the pad.
    pub fn effect(self) -> Option<Effect> {
        let templates = match self {
            CheatButton::RainWeapons => WEAPON_TEMPLATES,
            CheatButton::RainModules => MODULE_TEMPLATES,
            CheatButton::Done => return None,
        };
        Some(Effect::RainItems {
            template_ids: templates.to_vec(),
        })
    }
}

/// The canvas rect of the `index`th list row.
fn button_rect(rects: PanelRects, index: usize) -> Rect {
    let list = rects.list_rect();
    Rect::new(
        list.x + BUTTON_INSET,
        list.y + BUTTON_INSET + index as f32 * BUTTON_PITCH,
        list.w - 2.0 * BUTTON_INSET,
        BUTTON_H,
    )
}

/// The button at a canvas point, if any. Both the click and the hover
/// highlight go through this, so the two can never disagree.
fn hit(rects: PanelRects, point: Vector2<f32>) -> Option<CheatButton> {
    for (index, (button, _)) in CheatButton::ROWS.iter().enumerate() {
        if button_rect(rects, index).contains(point) {
            return Some(*button);
        }
    }
    rects
        .done_rect()
        .contains(point)
        .then_some(CheatButton::Done)
}

/// Describe the pad onto a canvas: backdrop, header, the two rain buttons and
/// "Done". One description for both presentations.
fn build_canvas(rects: PanelRects, pointer_canvas: Option<Vector2<f32>>) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
    canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

    let hovered = pointer_canvas.and_then(|point| hit(rects, point));
    let opacity = |button: CheatButton| {
        if hovered == Some(button) {
            HOVER_OPACITY
        } else {
            IDLE_OPACITY
        }
    };

    canvas.text_native(
        rects.header_rect(),
        "Cheats",
        MENU_FONT,
        HAlign::Center,
        VAlign::Middle,
    );

    for (index, (button, label)) in CheatButton::ROWS.iter().enumerate() {
        // Fitted, not plain: `text_native` does not shrink to its rect, so a
        // label that outgrows the pane would run over the frame instead of
        // ellipsizing inside it.
        canvas
            .text_native_fit(
                button_rect(rects, index),
                label,
                ROW_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(opacity(*button));
    }

    canvas
        .text_native(
            rects.done_rect(),
            "Done",
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        )
        .opacity(opacity(CheatButton::Done));

    canvas
}

/// The cheat overlay. Closed by default; [`CheatPad::open`] arms it.
pub struct CheatPad {
    open: bool,
    menu: FrontendMenu<CheatButton>,
    /// The head pose from the latest update, for the comfort dim (which
    /// follows the gaze rather than the world-locked panel).
    head: (Vector3<f32>, Quaternion<f32>),
    /// Set when a click closed the pad, and cleared once that click is
    /// released. See [`CheatPad::suspends_scene`].
    closed_under_a_held_press: bool,
    /// The panel's widget rects, re-resolved each update (the render path
    /// takes `&self`, so it reads them here).
    rects: PanelRects,
}

impl Default for CheatPad {
    fn default() -> Self {
        Self::new()
    }
}

impl CheatPad {
    pub fn new() -> Self {
        Self {
            open: false,
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
            head: UNTRACKED_HEAD,
            closed_under_a_held_press: false,
            rects: PanelRects::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the active scene must stay frozen this frame. Wider than
    /// [`is_open`](Self::is_open) by one beat, for the same reason the pause
    /// menu's is: the press that closed the pad is still down, and
    /// `VirtualHand` - never advanced while the pad was up - would read it as
    /// a fresh rising edge and fire the held weapon the instant the world came
    /// back.
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

    /// Open the pad in front of the player. `last_pressed` starts `true` (via
    /// `reset_on_entry`) so the very chord that opened it cannot read as a
    /// rising edge over a button on the first frame.
    pub fn open(&mut self) {
        self.open = true;
        self.menu.reset_on_entry();
        // Forget the previous session's head: a stale pose would hang the dim
        // where the player stood last time if `render` beats the first
        // `update`.
        self.head = UNTRACKED_HEAD;
    }

    /// Close because a button was clicked - the press that did it is still
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

    /// Advance the overlay and report the button that was clicked, if any.
    /// A no-op returning `None` while closed.
    pub fn update(
        &mut self,
        elapsed: std::time::Duration,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> Option<CheatButton> {
        if !self.open {
            return None;
        }
        self.rects = dev_params_panel::rects(asset_cache);
        self.head = (input_context.head.position, input_context.head.rotation);

        let rects = self.rects;
        self.menu.update(
            elapsed,
            input_context,
            options.presentation_mode,
            |point| hit(rects, point),
            |point| hit(rects, point),
        )
    }

    /// World-space presentation: the canvas on a panel in front of the player.
    /// Empty when closed or flat. `pawn_to_world` maps the tracked play space
    /// (where the head, the hands and therefore the panel live) into world
    /// coordinates, exactly as the pause menu does.
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
                let canvas = build_canvas(self.rects, self.menu.pointer_canvas());
                let (dim_position, dim_forward) = dim_pose(self.head.0, self.head.1, &panel);
                let mut objects = vec![world_dim_layer(
                    dim_position,
                    dim_forward,
                    dim_distance(dim_position, &panel),
                    crate::util::render_source::PAUSE_DIM,
                )];
                objects.extend(self.menu.render_world_space(
                    asset_cache,
                    canvas,
                    options.presentation_mode,
                ));
                for object in &mut objects {
                    object.set_transform(pawn_to_world * object.get_transform());
                }
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
                let canvas = build_canvas(self.rects, pointer_canvas);
                self.menu.render_screen_space(
                    asset_cache,
                    canvas,
                    screen_size,
                    options.presentation_mode,
                )
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every button is reachable, and the two rain rows do not overlap each
    /// other or the "Done" rect they share a backdrop with.
    #[test]
    fn each_button_hit_tests_to_itself() {
        let rects = PanelRects::default();
        assert_eq!(
            hit(rects, button_rect(rects, 0).center()),
            Some(CheatButton::RainWeapons)
        );
        assert_eq!(
            hit(rects, button_rect(rects, 1).center()),
            Some(CheatButton::RainModules)
        );
        assert_eq!(
            hit(rects, rects.done_rect().center()),
            Some(CheatButton::Done)
        );
    }

    #[test]
    fn the_rain_rows_stay_inside_the_list_pane() {
        let rects = PanelRects::default();
        let list = rects.list_rect();
        for index in 0..CheatButton::ROWS.len() {
            let row = button_rect(rects, index);
            assert!(
                row.x >= list.x && row.x + row.w <= list.x + list.w,
                "{row:?}"
            );
            assert!(
                row.y >= list.y && row.y + row.h <= list.y + list.h,
                "{row:?}"
            );
            assert_eq!(hit(rects, row.center()), Some(CheatButton::ROWS[index].0));
        }
    }

    /// The pad's whole point: each rain button asks for a spread of items, and
    /// the two spreads are different.
    #[test]
    fn each_rain_button_asks_for_a_spread_of_items() {
        let rained = |button: CheatButton| match button.effect() {
            Some(Effect::RainItems { template_ids }) => template_ids,
            other => panic!("{button:?} produced {other:?}"),
        };
        for button in [CheatButton::RainWeapons, CheatButton::RainModules] {
            let ids = rained(button);
            assert!(
                (6..=10).contains(&ids.len()),
                "{button:?} rains {} items - a modest shower, not a physics stress test",
                ids.len()
            );
            assert!(ids.iter().all(|id| *id < 0), "templates are negative ids");
        }
        assert_ne!(
            rained(CheatButton::RainWeapons),
            rained(CheatButton::RainModules)
        );
        assert!(CheatButton::Done.effect().is_none());
    }

    /// The press that clicked a button is still held on the frame the pad
    /// closes; resuming into it would read as a fresh trigger pull.
    #[test]
    fn the_scene_stays_suspended_until_the_selecting_press_is_released() {
        let mut pad = CheatPad::new();
        pad.open();
        assert!(pad.suspends_scene());

        pad.close_after_click();
        assert!(!pad.is_open());
        assert!(pad.suspends_scene(), "the click is still held");

        let mut held = InputContext::default();
        held.right_hand.trigger_value = 1.0;
        pad.poll_release(&held);
        assert!(pad.suspends_scene());

        pad.poll_release(&InputContext::default());
        assert!(!pad.suspends_scene(), "released: the world may run again");
    }

    /// Closing with the chord has nothing held to wait for, so it must not
    /// strand the simulation.
    #[test]
    fn closing_with_the_toggle_resumes_immediately() {
        let mut pad = CheatPad::new();
        pad.open();
        pad.close();
        assert!(!pad.suspends_scene());
    }

    /// The chord that opens the pad is a press: without `reset_on_entry`'s
    /// armed latch it would read as a rising edge over whatever the VR ray
    /// crossed on the very first frame.
    #[test]
    fn the_press_that_opens_the_pad_cannot_click_a_button() {
        let rects = PanelRects::default();
        let mut pad = CheatPad::new();
        pad.open();
        pad.rects = rects;
        let point = Some(button_rect(rects, 0).center());

        assert_eq!(
            pad.menu
                .resolve_pointer(point, true, |p| hit(rects, p), |p| hit(rects, p)),
            None,
            "a press carried in from the opening chord must not rain items"
        );
        // Releasing and pressing again is a real click.
        pad.menu
            .resolve_pointer(point, false, |p| hit(rects, p), |p| hit(rects, p));
        assert_eq!(
            pad.menu
                .resolve_pointer(point, true, |p| hit(rects, p), |p| hit(rects, p)),
            Some(CheatButton::RainWeapons)
        );
    }
}
