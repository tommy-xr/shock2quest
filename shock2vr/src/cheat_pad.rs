//! The developer cheat pad.
//!
//! A scrolling list of cheats that rain useful items around the player, for
//! setting up a test situation without playing the game up to it. Like
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
//! over `GAMELOD.PCX`) and scrolls with the shared [`list_scroll`] rocker, so
//! it is the same list the Developer screen's parameter page and debug-scene
//! launcher are: placement is resolved once, in canvas pixels, and flatscreen
//! and VR draw the identical canvas (AGENTS.md section 3). Adding a cheat is
//! one line in [`CHEATS`] - the page grows and starts scrolling on its own.

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
        dev_params_panel::{self, FIELD_TOP_Y, PanelRects},
        list_scroll::{self, ScrollHalf},
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
/// The face the Developer screen's own lists use, at their row pitch, so a
/// cheat row and a debug-scene row are the same row.
const ROW_FONT: &str = "mainfont.fon";
const ROW_H: f32 = 19.0;
/// Horizontal inset of a row's text from the row, matching the launcher's.
const TEXT_INSET: f32 = 8.0;
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Opacity for a row the pointer is not over, and for the one it is.
/// Matches [`dev_params_panel`]'s pair so the two pages highlight alike.
const IDLE_OPACITY: f32 = 0.65;
const HOVER_OPACITY: f32 = 1.0;

/// One entry in the list: the row's label and the templates it rains.
pub struct Cheat {
    pub label: &'static str,
    templates: &'static [i32],
}

/// Every cheat, in screen order. Template ids come from `cargo dq` against the
/// gamesys; a new cheat is one line here and nothing else - the list pages and
/// grows a scroll rocker on its own once the entries outrun the pane.
pub static CHEATS: &[Cheat] = &[
    Cheat {
        label: "Rain weapons",
        // The four workhorse weapons, plus a clip for each gun that takes one.
        templates: &[
            -928,  // Wrench
            -17,   // Pistol
            -19,   // Shotgun
            -18,   // Assault Rifle
            -1358, // Small Standard Clip
            -1358, // Small Standard Clip
            -1360, // Small AP Clip
            -42,   // Pellet Shot Box (shotgun shells)
        ],
    },
    Cheat {
        label: "Rain modules + nanites",
        // The two things a test situation is usually short of.
        templates: &[
            -938, // EXP Cookies (cyber modules)
            -938, -938, -938, //
            -89,  // 20 Nanites
            -89, -89, -89,
        ],
    },
];

impl Cheat {
    /// The effect this row asks the scene for.
    pub fn effect(&self) -> Effect {
        Effect::RainItems {
            template_ids: self.templates.to_vec(),
        }
    }
}

/// What a click on the pad resolves to. `Cheat` carries the index of the entry
/// scrolled into that row, never the row's own slot: a positional index would
/// rain whatever entry *used* to sit there the moment the list scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheatEvent {
    Cheat(usize),
    Scroll(ScrollHalf),
    /// Close the pad and let the simulation run again.
    Done,
}

/// What the owning `Game` must act on. Scrolling never reaches it - the pad
/// absorbs that itself, exactly as [`dev_params_panel::activate`] absorbs a
/// parameter step.
pub enum CheatOutcome {
    Rain(Effect),
    Close,
}

// The list geometry, all of it `list_scroll`'s. Parameterised by `len` rather
// than reading `CHEATS` directly so the paging and scroll behaviour is testable
// against a list longer than the two entries shipped today.

fn rows_per_page(rects: PanelRects) -> usize {
    list_scroll::rows_per_page(rects.list_rect(), FIELD_TOP_Y, ROW_H)
}

fn max_scroll(rects: PanelRects, len: usize) -> usize {
    list_scroll::max_scroll(len, rows_per_page(rects))
}

fn visible_rows(rects: PanelRects, len: usize, scroll: usize) -> std::ops::Range<usize> {
    list_scroll::visible_rows(len, rows_per_page(rects), scroll)
}

fn rocker(rects: PanelRects, len: usize) -> Option<list_scroll::Rocker> {
    list_scroll::rocker(rects.list_rect(), FIELD_TOP_Y, max_scroll(rects, len) > 0)
}

/// The canvas rect of the `slot`-th visible row. Rows stop short of the scroll
/// gutter whenever it is in use, so a row and the rocker can never claim the
/// same point.
fn row_rect(rects: PanelRects, len: usize, slot: usize) -> Rect {
    let list = rects.list_rect();
    let gutter = if max_scroll(rects, len) > 0 {
        list_scroll::GUTTER_W
    } else {
        0.0
    };
    Rect::new(
        list.x,
        list.y + slot as f32 * ROW_H,
        (list.w - gutter).max(0.0),
        ROW_H,
    )
}

/// The rect a row's label is drawn in: the row, inset on both edges.
fn text_rect(rects: PanelRects, len: usize, slot: usize) -> Rect {
    let row = row_rect(rects, len, slot);
    Rect::new(
        row.x + TEXT_INSET,
        row.y,
        (row.w - 2.0 * TEXT_INSET).max(0.0),
        row.h,
    )
}

/// What is at a canvas point, if any. Both the click and the hover highlight go
/// through this, so the two can never disagree.
fn hit(rects: PanelRects, len: usize, scroll: usize, point: Vector2<f32>) -> Option<CheatEvent> {
    if rects.done_rect().contains(point) {
        return Some(CheatEvent::Done);
    }
    let rows = visible_rows(rects, len, scroll);
    if let Some(rocker) = rocker(rects, len) {
        if let Some(half) = list_scroll::hit(&rocker, rows.start, max_scroll(rects, len), point) {
            return Some(CheatEvent::Scroll(half));
        }
    }
    (0..rows.len())
        .find(|slot| row_rect(rects, len, *slot).contains(point))
        .map(|slot| CheatEvent::Cheat(rows.start + slot))
}

/// Describe the pad onto a canvas: backdrop, header, the scrolled rows, the
/// rocker if the list needs one, and "Done". One description for both
/// presentations.
fn build_canvas(
    rects: PanelRects,
    len: usize,
    scroll: usize,
    pointer_canvas: Option<Vector2<f32>>,
) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
    canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

    let hovered = pointer_canvas.and_then(|point| hit(rects, len, scroll, point));
    let opacity = |event: CheatEvent| {
        if hovered == Some(event) {
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

    let rows = visible_rows(rects, len, scroll);
    for (slot, index) in rows.clone().enumerate() {
        // Fitted, not plain: `text_native` does not shrink to its rect, so a
        // label that outgrows the pane would run over the frame - and over the
        // scroll gutter - instead of ellipsizing inside it.
        canvas
            .text_native_fit(
                text_rect(rects, len, slot),
                CHEATS[index].label,
                ROW_FONT,
                HAlign::Left,
                VAlign::Middle,
            )
            .opacity(opacity(CheatEvent::Cheat(index)));
    }

    if let Some(rocker) = rocker(rects, len) {
        list_scroll::draw(
            &mut canvas,
            &rocker,
            rows.start,
            max_scroll(rects, len),
            match hovered {
                Some(CheatEvent::Scroll(half)) => Some(half),
                _ => None,
            },
        );
    }

    canvas
        .text_native(
            rects.done_rect(),
            "Done",
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        )
        .opacity(opacity(CheatEvent::Done));

    canvas
}

/// The cheat overlay. Closed by default; [`CheatPad::open`] arms it.
pub struct CheatPad {
    open: bool,
    menu: FrontendMenu<CheatEvent>,
    /// The head pose from the latest update, for the comfort dim (which
    /// follows the gaze rather than the world-locked panel).
    head: (Vector3<f32>, Quaternion<f32>),
    /// Set when a click closed the pad, and cleared once that click is
    /// released. See [`CheatPad::suspends_scene`].
    closed_under_a_held_press: bool,
    /// The panel's widget rects, re-resolved each update (the render path
    /// takes `&self`, so it reads them here).
    rects: PanelRects,
    /// Index of the cheat in the top row.
    scroll: usize,
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
            scroll: 0,
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
    /// rising edge over a row on the first frame.
    pub fn open(&mut self) {
        self.open = true;
        self.menu.reset_on_entry();
        // Always land at the top of the list: reopening part-way down one the
        // player forgot they had scrolled would read as a broken page.
        self.scroll = 0;
        // Forget the previous session's head: a stale pose would hang the dim
        // where the player stood last time if `render` beats the first
        // `update`.
        self.head = UNTRACKED_HEAD;
    }

    /// Close because a row was clicked - the press that did it is still held,
    /// so latch the scene shut until it is released.
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

    /// Advance the overlay and report what the owning `Game` must act on, if
    /// anything. A no-op returning `None` while closed.
    pub fn update(
        &mut self,
        elapsed: std::time::Duration,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> Option<CheatOutcome> {
        if !self.open {
            return None;
        }
        self.rects = dev_params_panel::rects(asset_cache);
        self.head = (input_context.head.position, input_context.head.rotation);

        let (rects, len, scroll) = (self.rects, CHEATS.len(), self.scroll);
        let event = self.menu.update(
            elapsed,
            input_context,
            options.presentation_mode,
            |point| hit(rects, len, scroll, point),
            |point| hit(rects, len, scroll, point),
        );
        self.activate(event)
    }

    /// Apply a clicked event: scrolling is absorbed here, the rest goes up.
    fn activate(&mut self, event: Option<CheatEvent>) -> Option<CheatOutcome> {
        match event? {
            CheatEvent::Cheat(index) => Some(CheatOutcome::Rain(CHEATS[index].effect())),
            CheatEvent::Scroll(half) => {
                list_scroll::apply(half, &mut self.scroll, max_scroll(self.rects, CHEATS.len()));
                None
            }
            CheatEvent::Done => Some(CheatOutcome::Close),
        }
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
                let canvas = build_canvas(
                    self.rects,
                    CHEATS.len(),
                    self.scroll,
                    self.menu.pointer_canvas(),
                );
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
                let canvas = build_canvas(self.rects, CHEATS.len(), self.scroll, pointer_canvas);
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

    /// A list long enough to outrun the pane, for the scrolling assertions.
    /// The shipped `CHEATS` fits on one page today - which is the point of
    /// parameterising the geometry on `len` rather than reading it.
    fn long_list(rects: PanelRects) -> usize {
        rows_per_page(rects) + 5
    }

    #[test]
    fn every_shipped_cheat_is_reachable_and_hit_tests_to_itself() {
        let rects = PanelRects::default();
        let len = CHEATS.len();
        assert!(
            len <= rows_per_page(rects),
            "the shipped list fits one page"
        );
        for index in 0..len {
            assert_eq!(
                hit(rects, len, 0, row_rect(rects, len, index).center()),
                Some(CheatEvent::Cheat(index)),
            );
        }
        assert_eq!(
            hit(rects, len, 0, rects.done_rect().center()),
            Some(CheatEvent::Done)
        );
    }

    #[test]
    fn the_rows_stay_inside_the_list_pane() {
        let rects = PanelRects::default();
        let len = CHEATS.len();
        let list = rects.list_rect();
        for slot in 0..len {
            let row = row_rect(rects, len, slot);
            assert!(
                row.x >= list.x && row.x + row.w <= list.x + list.w,
                "{row:?}"
            );
            assert!(
                row.y >= list.y && row.y + row.h <= list.y + list.h,
                "{row:?}"
            );
        }
    }

    /// The list is meant to grow. Once it outruns the pane a rocker appears,
    /// the rows make room for its gutter, and a row reports the entry
    /// *scrolled into* it rather than its own slot.
    #[test]
    fn a_list_that_outruns_the_pane_scrolls_and_rows_follow_the_offset() {
        let rects = PanelRects::default();
        let len = long_list(rects);
        let max = max_scroll(rects, len);
        assert_eq!(max, 5);

        let no_gutter = row_rect(rects, CHEATS.len(), 0);
        let with_gutter = row_rect(rects, len, 0);
        assert_eq!(
            with_gutter.w,
            no_gutter.w - list_scroll::GUTTER_W,
            "rows must clear the rocker's gutter once it appears"
        );

        // Unscrolled the top row is entry 0; scrolled by 3 it is entry 3.
        assert_eq!(
            hit(rects, len, 0, row_rect(rects, len, 0).center()),
            Some(CheatEvent::Cheat(0))
        );
        assert_eq!(
            hit(rects, len, 3, row_rect(rects, len, 0).center()),
            Some(CheatEvent::Cheat(3))
        );
        // An over-scroll cannot walk the list off its end.
        assert_eq!(
            hit(rects, len, 99, row_rect(rects, len, 0).center()),
            Some(CheatEvent::Cheat(max))
        );
    }

    /// A short list has no rocker at all, so nothing can steal a row's clicks.
    #[test]
    fn a_list_that_fits_has_no_rocker() {
        let rects = PanelRects::default();
        assert!(rocker(rects, CHEATS.len()).is_none());
        assert!(rocker(rects, long_list(rects)).is_some());
    }

    /// The rocker's ends are inert, and clicking it scrolls rather than
    /// reaching `Game` - the pad absorbs it.
    #[test]
    fn the_rocker_scrolls_and_stops_at_both_ends() {
        let rects = PanelRects::default();
        let len = long_list(rects);
        let max = max_scroll(rects, len);
        let rocker = rocker(rects, len).expect("this list scrolls");

        // At the top, "up" is inert and "down" scrolls.
        assert_eq!(hit(rects, len, 0, rocker.up.center()), None);
        assert_eq!(
            hit(rects, len, 0, rocker.down.center()),
            Some(CheatEvent::Scroll(ScrollHalf::Down))
        );

        let mut pad = CheatPad::new();
        pad.rects = rects;
        // `activate` scrolls against the SHIPPED list, which fits, so its max
        // is zero - assert the absorption, then the walk on the long list.
        assert!(matches!(
            pad.activate(Some(CheatEvent::Scroll(ScrollHalf::Down))),
            None
        ));

        let mut scroll = 0;
        for _ in 0..max + 3 {
            list_scroll::apply(ScrollHalf::Down, &mut scroll, max);
        }
        assert_eq!(scroll, max);
        assert_eq!(hit(rects, len, max, rocker.down.center()), None);
        assert_eq!(
            hit(rects, len, max, rocker.up.center()),
            Some(CheatEvent::Scroll(ScrollHalf::Up))
        );
    }

    /// The pad's whole point: each cheat asks for a spread of items, and the
    /// spreads differ.
    #[test]
    fn each_cheat_asks_for_a_spread_of_items() {
        let rained = |cheat: &Cheat| match cheat.effect() {
            Effect::RainItems { template_ids } => template_ids,
            other => panic!("{} produced {other:?}", cheat.label),
        };
        for cheat in CHEATS {
            let ids = rained(cheat);
            assert!(
                (6..=10).contains(&ids.len()),
                "{} rains {} items - a modest shower, not a physics stress test",
                cheat.label,
                ids.len()
            );
            assert!(ids.iter().all(|id| *id < 0), "templates are negative ids");
        }
        assert_ne!(rained(&CHEATS[0]), rained(&CHEATS[1]));
    }

    /// The press that clicked a row is still held on the frame the pad closes;
    /// resuming into it would read as a fresh trigger pull.
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

    /// Reopening lands at the top of the list rather than wherever it was left.
    #[test]
    fn reopening_resets_the_scroll() {
        let mut pad = CheatPad::new();
        pad.open();
        pad.scroll = 4;
        pad.close();
        pad.open();
        assert_eq!(pad.scroll, 0);
    }

    /// The chord that opens the pad is a press: without `reset_on_entry`'s
    /// armed latch it would read as a rising edge over whatever the VR ray
    /// crossed on the very first frame.
    #[test]
    fn the_press_that_opens_the_pad_cannot_click_a_row() {
        let rects = PanelRects::default();
        let len = CHEATS.len();
        let mut pad = CheatPad::new();
        pad.open();
        pad.rects = rects;
        let point = Some(row_rect(rects, len, 0).center());
        let at = |p| hit(rects, len, 0, p);

        assert_eq!(
            pad.menu.resolve_pointer(point, true, at, at),
            None,
            "a press carried in from the opening chord must not rain items"
        );
        // Releasing and pressing again is a real click.
        pad.menu.resolve_pointer(point, false, at, at);
        assert_eq!(
            pad.menu.resolve_pointer(point, true, at, at),
            Some(CheatEvent::Cheat(0))
        );
    }
}
