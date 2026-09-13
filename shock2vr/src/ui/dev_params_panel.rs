//! The Developer screen's parameter rows, described once for every host.
//!
//! One row per [`crate::dev_params`] entry - label, `<` button, value readout,
//! `>` button - plus a "Done" button, all in canvas pixels on the shared
//! 640x480 frontend canvas. Two hosts draw it (the [`DeveloperScene`] reached
//! from the main menu, and the pause overlay's Developer page) and both call
//! these same functions, so the screen cannot drift between hosts - and
//! because placement is decided here, once, in canvas pixels, flatscreen and
//! VR render it identically by construction (AGENTS.md §3).
//!
//! The geometry rides the `GAMELOD.PCX` backdrop both hosts use for this
//! page: the header line, the dark list pane, and the framed button art in
//! the bottom-right corner (the decoded `GAMELODR.BIN` rects, shared with
//! [`crate::scenes::LoadGameScene`]). Rows live inside the pane, scrolling in
//! a gutter down its right edge so a registry larger than the pane stays
//! fully reachable; "Done" sits on the button art.
//!
//! [`DeveloperScene`]: crate::scenes::DeveloperScene

use std::ops::Range;

use cgmath::{Vector2, vec2};
use dark::{importers::UI_LAYOUT_IMPORTER, map::MapRect};
use engine::assets::asset_cache::AssetCache;

use super::{
    HAlign, Rect, UiCanvas, VAlign,
    list_scroll::{self, ScrollHalf},
};
use crate::dev_params::{self, DevParamId, DevParamKind};

/// The frontend screens are authored on the original 640x480 canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;

/// Display font for the header, the arrows and "Done" (`METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// Small data font for the row labels and value readouts (`mainfont.fon`).
const ROW_FONT: &str = "mainfont.fon";

/// The backdrop this panel is laid out on, and its widget-rect layout file.
/// Shared with [`crate::scenes::LoadGameScene`], which owns the same art.
pub const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
const LAYOUT_FILE: &str = "GAMELODR.BIN";

/// Indices into `GAMELODR.BIN`: header line, the dark list pane, the upper
/// framed button (the load screen's "Load"; the Developer screen puts its
/// debug-scene launcher there) and the bottom-right button art.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const ACTION_RECT_INDEX: usize = 2;
const DONE_RECT_INDEX: usize = 3;

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent - the
/// same fallbacks the load screen carries for the same art.
const FALLBACK_HEADER: Rect = Rect::new(261.0, 31.0, 202.0, 20.0);
const FALLBACK_LIST: Rect = Rect::new(261.0, 54.0, 202.0, 290.0);
const FALLBACK_ACTION: Rect = Rect::new(527.0, 161.0, 96.0, 62.0);
const FALLBACK_DONE: Rect = Rect::new(527.0, 405.0, 95.0, 62.0);

/// Canvas y where the backdrop paints its bordered name-entry field; rows
/// stop above it (see the sibling constant on the load screen). Public so the
/// other page drawn on this backdrop - the Developer screen's debug-scene
/// launcher - stops its own rows at the same painted border.
pub const FIELD_TOP_Y: f32 = 323.0;

/// The panel's widget rects, resolved from `GAMELODR.BIN`.
///
/// Read from the layout file rather than hardcoded, for the same reason the
/// load screen reads them: they describe where the *art* puts its widgets, so
/// an alternate authored layout has to move the rows, the arrows and "Done"
/// with the backdrop. Both hosts resolve this once per frame and hand it to
/// [`draw`] and [`hit`] alike, so drawing and hit-testing can never disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRects {
    header: Rect,
    list: Rect,
    action: Rect,
    done: Rect,
}

impl Default for PanelRects {
    fn default() -> Self {
        Self {
            header: FALLBACK_HEADER,
            list: FALLBACK_LIST,
            action: FALLBACK_ACTION,
            done: FALLBACK_DONE,
        }
    }
}

impl PanelRects {
    fn from_layout(layout: Option<&[MapRect]>) -> Self {
        let at = |index: usize, fallback: Rect| {
            layout
                .and_then(|rects| rects.get(index))
                .map(|r| {
                    Rect::new(
                        r.ul_x as f32,
                        r.ul_y as f32,
                        r.width() as f32,
                        r.height() as f32,
                    )
                })
                .unwrap_or(fallback)
        };
        Self {
            header: at(HEADER_RECT_INDEX, FALLBACK_HEADER),
            list: at(LIST_RECT_INDEX, FALLBACK_LIST),
            action: at(ACTION_RECT_INDEX, FALLBACK_ACTION),
            done: at(DONE_RECT_INDEX, FALLBACK_DONE),
        }
    }

    /// Canvas center of the "Done" button, for hosts that need to reason
    /// about where it lands relative to their own widgets (the pause overlay
    /// asserts that its "Quit" button sits under it).
    pub fn done_center(&self) -> Vector2<f32> {
        self.done.center()
    }

    /// The header line, the list pane and the upper framed button, for a host
    /// that draws its own page on this same backdrop (the Developer screen's
    /// debug-scene launcher). Read here rather than re-resolved so every page
    /// of that screen rides one set of authored rects.
    pub fn header_rect(&self) -> Rect {
        self.header
    }

    pub fn list_rect(&self) -> Rect {
        self.list
    }

    pub fn action_rect(&self) -> Rect {
        self.action
    }

    pub fn done_rect(&self) -> Rect {
        self.done
    }
}

/// Resolve the panel's rects from the shipped layout file.
pub fn rects(asset_cache: &mut AssetCache) -> PanelRects {
    let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
    PanelRects::from_layout(layout.as_deref().map(|r| r.as_slice()))
}

/// Vertical distance between row tops, and each row's own height. Taller
/// than the load list's 19px rows: these rows carry click targets (the
/// arrows), so they get more air and a bigger hit area.
///
/// These were briefly shaved to 25/22 to squeeze a tenth parameter into the
/// pane. That trade is gone: the list scrolls, so the pane holds whatever it
/// comfortably can and the registry may grow without touching the pitch.
const ROW_PITCH: f32 = 28.0;
const ROW_H: f32 = 24.0;
/// Horizontal inset from the pane's edges, matching the load list's text
/// inset so the two screens' contents align inside the same art.
const TEXT_INSET: f32 = 8.0;
/// Width of the `<` / `>` hit regions.
const ARROW_W: f32 = 20.0;
/// Width of the value readout between the arrows.
const VALUE_W: f32 = 48.0;
/// The scroll gutter down the list pane's right edge. The rocker itself - its
/// geometry, its arrow art and its "an end that cannot move is inert" rule - is
/// [`list_scroll`]'s, shared with the debug-scene launcher.
const SCROLL_GUTTER_W: f32 = list_scroll::GUTTER_W;

/// Opacity for an element the pointer is not over.
const IDLE_OPACITY: f32 = 0.65;
/// Opacity for the element under the pointer.
const HOVER_OPACITY: f32 = 1.0;
/// Labels and values are readouts, not click targets: drawn steady, between
/// the two interactive levels, so the hover flow reads on the arrows.
const READOUT_OPACITY: f32 = 0.85;

/// What a click on the panel asks for. `Decrement`/`Increment` step the
/// parameter by its own declared step; `Done` leaves the screen (the host
/// decides where "back" goes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevParamsEvent {
    Decrement(DevParamId),
    Increment(DevParamId),
    /// Scroll the list one row towards the top of the registry.
    ScrollUp,
    /// Scroll the list one row towards its end.
    ScrollDown,
    Done,
}

/// A row's resolved rects, all derived from the shared layout constants.
struct RowRects {
    label: Rect,
    decrement: Rect,
    value: Rect,
    increment: Rect,
}

fn row_rects(rects: PanelRects, index: usize) -> RowRects {
    let list = rects.list;
    let y = list.y + index as f32 * ROW_PITCH;
    // Leave the scroll gutter clear whenever it is in use, so a row's `>`
    // never sits under the rocker (they resolve through one hit test, so an
    // overlap is a genuine ambiguity, not just a visual one).
    // The gutter subsumes the right inset: the rocker's own art carries a dark
    // margin either side of its arrow, so charging the row for both as well
    // costs label width and ellipsizes names that otherwise fit.
    let right_margin = if max_scroll(rects) > 0 {
        SCROLL_GUTTER_W
    } else {
        TEXT_INSET
    };
    let right = list.x + list.w - right_margin;
    let increment_x = right - ARROW_W;
    let value_x = increment_x - VALUE_W;
    let decrement_x = value_x - ARROW_W;
    let label_x = list.x + TEXT_INSET;
    RowRects {
        label: Rect::new(label_x, y, decrement_x - label_x, ROW_H),
        decrement: Rect::new(decrement_x, y, ARROW_W, ROW_H),
        value: Rect::new(value_x, y, VALUE_W, ROW_H),
        increment: Rect::new(increment_x, y, ARROW_W, ROW_H),
    }
}

/// This page's list geometry - the paging and the gutter rocker every other
/// frontend list uses, at the parameter rows' own pitch. The rows themselves
/// are built by [`row_rects`] rather than the shared `row_rect`, because a
/// parameter row is not one target but four columns (label, `<`, value, `>`).
fn list(rects: PanelRects) -> list_scroll::ListGeometry {
    list_scroll::ListGeometry {
        pane: rects.list,
        bottom_limit: FIELD_TOP_Y,
        row_h: ROW_PITCH,
        text_inset: TEXT_INSET,
    }
}

/// How many rows fit in the pane above the backdrop's painted field - the
/// size of one page of the list, however long the registry is.
fn rows_per_page(rects: PanelRects) -> usize {
    list(rects).rows_per_page()
}

/// The furthest the list can scroll: the first-row index that puts the tail
/// of the registry against the bottom of the pane. Zero when everything fits
/// at once, which is also what hides the scroll rocker.
fn max_scroll(rects: PanelRects) -> usize {
    list(rects).max_scroll(dev_params::PARAMS.len())
}

/// The registry indices on screen at `scroll`, with `scroll` clamped to what
/// the pane can actually show.
///
/// The one place a screen row is tied to a parameter: [`draw`] and [`hit`]
/// both walk this range, so a row can never *show* one parameter's value and
/// *step* another's - the failure a positional row index invites the moment
/// the list scrolls.
fn visible_rows(rects: PanelRects, scroll: usize) -> Range<usize> {
    list(rects).visible_rows(dev_params::PARAMS.len(), scroll)
}

/// The scroll rocker's two halves - up and down arrows - in a gutter down the
/// list pane's right edge, beside the rows they scroll. `None` when the whole
/// registry fits on one page.
///
/// Deliberately *not* on the upper framed button: that is the load screen's
/// "Load" frame, which the Developer screen now spends on its debug-scene
/// launcher ([`ACTION_RECT_INDEX`]). Scrolling belongs against its own list
/// anyway - a scrollbar's place is beside what it scrolls, not across the
/// screen from it.
fn rocker(rects: PanelRects) -> Option<list_scroll::Rocker> {
    list(rects).rocker(dev_params::PARAMS.len())
}

#[cfg(test)]
fn scroll_rects(rects: PanelRects) -> Option<(Rect, Rect)> {
    rocker(rects).map(|r| (r.up, r.down))
}

/// The value readout: floats as `{:.2}`, the format the step grids are
/// declared in.
fn format_value(kind: &DevParamKind, value: f32, bool_labels: Option<[&str; 2]>) -> String {
    match kind {
        DevParamKind::Float { .. } => format!("{value:.2}"),
        DevParamKind::Bool => {
            bool_labels.unwrap_or(["Off", "On"])[usize::from(value != 0.0)].to_owned()
        }
    }
}

/// The event at a canvas point, if any. Shared by the click and the hover
/// highlight, so the two can never disagree about where a button is.
pub fn hit(rects: PanelRects, scroll: usize, point: Vector2<f32>) -> Option<DevParamsEvent> {
    let mut canvas = UiCanvas::<DevParamsEvent>::with_events(vec2(CANVAS_W, CANVAS_H));
    // Rows are addressed by *screen slot* but carry the id of the registry
    // entry scrolled into that slot. Resolving the slot from anything other
    // than this same offset is the bug this whole seam invites: the panel
    // would draw one parameter and the click would change another.
    let rows = visible_rows(rects, scroll);
    for (slot, (id, _)) in dev_params::all()
        .skip(rows.start)
        .take(rows.len())
        .enumerate()
    {
        let row = row_rects(rects, slot);
        canvas.button(row.decrement, "", DevParamsEvent::Decrement(id));
        canvas.button(row.increment, "", DevParamsEvent::Increment(id));
    }
    canvas.button(rects.done, "", DevParamsEvent::Done);
    // A rocker half that cannot move is inert, not just dimmed - the rule lives
    // in `list_scroll`, so every scrolling list obeys the same one.
    if let Some(rocker) = rocker(rects) {
        if let Some(half) = list_scroll::hit(&rocker, rows.start, max_scroll(rects), point) {
            return Some(match half {
                ScrollHalf::Up => DevParamsEvent::ScrollUp,
                ScrollHalf::Down => DevParamsEvent::ScrollDown,
            });
        }
    }
    canvas.click_at(point)
}

/// Describe the panel onto the host's canvas: header, one row per parameter,
/// and "Done". `pointer_canvas` is the hover position in canvas pixels,
/// whatever produced it - the mouse or a VR controller ray; the highlight
/// resolves through the very same [`hit`] the click does.
pub fn draw(
    canvas: &mut UiCanvas,
    rects: PanelRects,
    scroll: usize,
    pointer_canvas: Option<Vector2<f32>>,
) {
    let hovered = pointer_canvas.and_then(|point| hit(rects, scroll, point));
    let hover_opacity = |event: DevParamsEvent| {
        if hovered == Some(event) {
            HOVER_OPACITY
        } else {
            IDLE_OPACITY
        }
    };

    canvas.text_native(
        rects.header,
        "Developer",
        MENU_FONT,
        HAlign::Center,
        VAlign::Middle,
    );

    let rows = visible_rows(rects, scroll);
    for (slot, (id, param)) in dev_params::all()
        .skip(rows.start)
        .take(rows.len())
        .enumerate()
    {
        let row = row_rects(rects, slot);
        // Fitted, not plain: a label is authored text of unbounded length and
        // `text_native` does not shrink to its rect, so a long one ("FOV
        // override (deg)", "Melee glove overlay") ran past the label column
        // and struck the `<` arrow beside it. Ellipsizing keeps the row
        // legible and the arrow clickable; the registry is free to name a
        // parameter clearly without measuring it first.
        canvas
            .text_native_fit(
                row.label,
                param.label,
                ROW_FONT,
                HAlign::Left,
                VAlign::Middle,
            )
            .opacity(READOUT_OPACITY);
        canvas
            .text_native(
                row.decrement,
                "<",
                MENU_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(hover_opacity(DevParamsEvent::Decrement(id)));
        canvas
            .text_native(
                row.value,
                &format_value(&param.kind, dev_params::get(id), param.bool_labels),
                ROW_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(READOUT_OPACITY);
        canvas
            .text_native(
                row.increment,
                ">",
                MENU_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(hover_opacity(DevParamsEvent::Increment(id)));
    }

    if let Some(rocker) = rocker(rects) {
        list_scroll::draw(
            canvas,
            &rocker,
            rows.start,
            max_scroll(rects),
            match hovered {
                Some(DevParamsEvent::ScrollUp) => Some(ScrollHalf::Up),
                Some(DevParamsEvent::ScrollDown) => Some(ScrollHalf::Down),
                _ => None,
            },
        );
    }

    canvas
        .text_native(
            rects.done,
            "Done",
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        )
        .opacity(hover_opacity(DevParamsEvent::Done));
}

/// Apply a clicked event to the registry, or to the host's `scroll` offset.
/// Returns `true` when the event was [`DevParamsEvent::Done`] - the one thing
/// the host must act on (leave the screen); the steps and the scrolling are
/// absorbed here so both hosts stay a one-liner.
pub fn activate(rects: PanelRects, event: DevParamsEvent, scroll: &mut usize) -> bool {
    let step_by = |id: DevParamId, direction: f32| match dev_params::spec(id).kind {
        DevParamKind::Float { step, .. } => {
            // `set` clamps into range and snaps to the step grid, so walking
            // off either end just pins to it.
            dev_params::set(id, dev_params::get(id) + direction * step);
        }
        // A switch has no grid to walk: either arrow flips it, so the row
        // behaves the same whichever side the pointer lands on.
        DevParamKind::Bool => {
            dev_params::set(id, if dev_params::get_bool(id) { 0.0 } else { 1.0 });
        }
    };
    match event {
        DevParamsEvent::Decrement(id) => {
            step_by(id, -1.0);
            false
        }
        DevParamsEvent::Increment(id) => {
            step_by(id, 1.0);
            false
        }
        DevParamsEvent::ScrollUp => {
            list_scroll::apply(ScrollHalf::Up, scroll, max_scroll(rects));
            false
        }
        DevParamsEvent::ScrollDown => {
            list_scroll::apply(ScrollHalf::Down, scroll, max_scroll(rects));
            false
        }
        DevParamsEvent::Done => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param_ids() -> Vec<DevParamId> {
        dev_params::all().map(|(id, _)| id).collect()
    }

    /// Every registered parameter must be *reachable*: whatever the registry
    /// grows to, some scroll offset puts it on screen with working arrows.
    /// (Its predecessor asserted every param fit on one page, which is what
    /// forced a pitch shave per new knob; the list scrolls now, so the
    /// invariant is reachability, not fitting.)
    #[test]
    fn every_registered_param_is_reachable_by_scrolling() {
        let rects = PanelRects::default();
        let ids = param_ids();
        for (index, id) in ids.iter().enumerate() {
            // Scrolling to a param's own index always shows it (clamped when
            // it is inside the last page).
            let scroll = index;
            let rows = visible_rows(rects, scroll);
            assert!(
                rows.contains(&index),
                "param {index} is not on screen at scroll {scroll}"
            );
            let row = row_rects(rects, index - rows.start);
            assert_eq!(
                hit(rects, scroll, row.increment.center()),
                Some(DevParamsEvent::Increment(*id)),
                "param {index} > at scroll {scroll}"
            );
        }
    }

    /// The last parameter in particular: it is only reachable at the bottom
    /// of the scroll, which is the offset a clamp bug lands one short of.
    #[test]
    fn the_last_param_is_reachable_at_the_bottom_of_the_scroll() {
        let rects = PanelRects::default();
        let last_index = dev_params::PARAMS.len() - 1;
        let last_id = *param_ids().last().unwrap();
        let bottom = max_scroll(rects);
        let rows = visible_rows(rects, bottom);
        assert_eq!(rows.end, dev_params::PARAMS.len());
        let row = row_rects(rects, last_index - rows.start);
        assert_eq!(
            hit(rects, bottom, row.decrement.center()),
            Some(DevParamsEvent::Decrement(last_id))
        );
        // ...and every drawn row clears the backdrop's painted field.
        assert!(row.label.y + ROW_H <= FIELD_TOP_Y);
        // An over-scroll cannot walk the list off its end.
        assert_eq!(visible_rows(rects, bottom + 99), rows);
    }

    /// The bug a scrolling list invites: the rows move but the hit test still
    /// indexes the registry positionally, so clicking a row steps whatever
    /// parameter *used* to be there. Slot 0 must belong to the first VISIBLE
    /// param, not to `PARAMS[0]`.
    #[test]
    fn the_hit_test_follows_the_scroll_offset() {
        let rects = PanelRects::default();
        let ids = param_ids();
        assert!(max_scroll(rects) > 0, "the pane must be scrollable to test");
        for scroll in 1..=max_scroll(rects) {
            let slot0 = row_rects(rects, 0);
            assert_eq!(
                hit(rects, scroll, slot0.increment.center()),
                Some(DevParamsEvent::Increment(ids[scroll])),
                "top row at scroll {scroll}"
            );
            assert_eq!(
                hit(rects, scroll, slot0.decrement.center()),
                Some(DevParamsEvent::Decrement(ids[scroll])),
                "top row at scroll {scroll}"
            );
            // The param that was on top before is now off screen entirely.
            assert!(!visible_rows(rects, scroll).contains(&(scroll - 1)));
        }
    }

    #[test]
    fn the_rocker_scrolls_and_stops_at_both_ends() {
        let rects = PanelRects::default();
        let (up, down) = scroll_rects(rects).expect("the shipped registry scrolls");
        let mut scroll = 0;

        // At the top the up half is inert - and stays inert if activated.
        assert_eq!(hit(rects, scroll, up.center()), None);
        assert_eq!(
            hit(rects, scroll, down.center()),
            Some(DevParamsEvent::ScrollDown)
        );
        assert!(!activate(rects, DevParamsEvent::ScrollUp, &mut scroll));
        assert_eq!(scroll, 0);

        // Walking down stops at the bottom rather than scrolling past the end.
        for _ in 0..max_scroll(rects) + 3 {
            activate(rects, DevParamsEvent::ScrollDown, &mut scroll);
        }
        assert_eq!(scroll, max_scroll(rects));
        assert_eq!(hit(rects, scroll, down.center()), None);
        assert_eq!(
            hit(rects, scroll, up.center()),
            Some(DevParamsEvent::ScrollUp)
        );

        activate(rects, DevParamsEvent::ScrollUp, &mut scroll);
        assert_eq!(scroll, max_scroll(rects) - 1);
    }

    /// The registry no longer fits any pane this backdrop can authorize, so
    /// the rocker is always present - even on the tallest pane the canvas
    /// allows.
    ///
    /// This used to assert the opposite ("a pane tall enough has no rocker").
    /// Rows are capped at `FIELD_TOP_Y / ROW_PITCH` = 11 however tall the
    /// authored pane is, and the table passed that when the free-camera
    /// switches landed. The fits-on-one-page branch is still live code and
    /// still covered, generically, by
    /// `list_scroll::the_rocker_sits_in_the_gutter_and_its_ends_are_inert`.
    #[test]
    fn the_tallest_authored_pane_still_needs_the_rocker() {
        let tall = PanelRects::from_layout(Some(&[
            MapRect::new(261, 31, 463, 51),
            // Starts at the top of the canvas, so it reaches FIELD_TOP_Y -
            // the most rows any authored layout can get.
            MapRect::new(261, 0, 463, 320),
            MapRect::new(527, 161, 623, 223),
            MapRect::new(527, 405, 622, 467),
        ]));
        let cap = (FIELD_TOP_Y / ROW_PITCH).floor() as usize;
        assert_eq!(rows_per_page(tall), cap);
        assert!(
            dev_params::PARAMS.len() > cap,
            "if the registry ever fits again, restore the no-rocker assertions"
        );
        assert!(max_scroll(tall) > 0);
        assert!(scroll_rects(tall).is_some());
        // Scrolling means the gutter is reserved: a row's increment stops at
        // the rocker's left edge. The gutter replaces the right inset rather
        // than adding to it, so reserving it costs the label no width it did
        // not already lose.
        assert_eq!(
            row_rects(tall, 0).increment.x + ARROW_W,
            tall.list.x + tall.list.w - SCROLL_GUTTER_W
        );
        assert_eq!(visible_rows(tall, 0), 0..cap);
    }

    #[test]
    fn rows_stay_inside_the_pane_horizontally() {
        let rects = PanelRects::default();
        let row = row_rects(rects, 0);
        assert!(row.label.x >= rects.list.x);
        assert!(
            row.increment.x + row.increment.w <= rects.list.x + rects.list.w,
            "the increment arrow must not spill out of the pane art"
        );
        // Left to right: label, <, value, >, with no overlaps.
        assert!(row.label.x + row.label.w <= row.decrement.x);
        assert!(row.decrement.x + row.decrement.w <= row.value.x);
        assert!(row.value.x + row.value.w <= row.increment.x);
    }

    #[test]
    fn the_arrows_and_done_hit_test() {
        let rects = PanelRects::default();
        let ids = param_ids();
        // Unscrolled, slot N is param N for every row the pane shows.
        for (index, id) in ids.iter().enumerate().take(rows_per_page(rects)) {
            let row = row_rects(rects, index);
            assert_eq!(
                hit(rects, 0, row.decrement.center()),
                Some(DevParamsEvent::Decrement(*id)),
                "row {index} <"
            );
            assert_eq!(
                hit(rects, 0, row.increment.center()),
                Some(DevParamsEvent::Increment(*id)),
                "row {index} >"
            );
            // The label and the value are readouts, not buttons.
            assert_eq!(hit(rects, 0, row.label.center()), None);
            assert_eq!(hit(rects, 0, row.value.center()), None);
        }
        assert_eq!(
            hit(rects, 0, rects.done.center()),
            Some(DevParamsEvent::Done)
        );
        // Bare backdrop is not a control.
        assert_eq!(hit(rects, 0, vec2(50.0, 50.0)), None);
    }

    /// The rows follow the layout FILE, not the decoded fallbacks: an
    /// alternate authored `GAMELODR.BIN` has to move the rows, the arrows and
    /// "Done" with the backdrop art, exactly as it moves the load screen's.
    #[test]
    fn the_layout_file_moves_the_rows_and_done() {
        let layout = [
            MapRect {
                ul_x: 10,
                ul_y: 0,
                lr_x: 110,
                lr_y: 50,
            },
            MapRect {
                ul_x: 20,
                ul_y: 60,
                lr_x: 300,
                lr_y: 300,
            },
            MapRect {
                ul_x: 350,
                ul_y: 100,
                lr_x: 396,
                lr_y: 160,
            },
            MapRect {
                ul_x: 400,
                ul_y: 400,
                lr_x: 500,
                lr_y: 450,
            },
        ];
        let rects = PanelRects::from_layout(Some(&layout));
        assert_eq!(rects.header, Rect::new(10.0, 0.0, 100.0, 50.0));
        assert_eq!(rects.done, Rect::new(400.0, 400.0, 100.0, 50.0));
        // Row 0 starts at the authored pane, and "Done" hit-tests where the
        // file put it - not at the fallback rect.
        let row = row_rects(rects, 0);
        assert_eq!(row.label.x, 20.0 + TEXT_INSET);
        assert_eq!(row.label.y, 60.0);
        assert_eq!(
            hit(rects, 0, rects.done.center()),
            Some(DevParamsEvent::Done)
        );
        assert_eq!(hit(rects, 0, FALLBACK_DONE.center()), None);
        // The rocker rides the authored *list pane*, so a moved backdrop takes
        // it along with the rows it scrolls - and the rows shorten to keep
        // their `>` out from under it.
        let (up, down) = scroll_rects(rects).expect("this pane is too short to fit the registry");
        let list_right = 20.0 + 280.0;
        assert_eq!(up.x, list_right - SCROLL_GUTTER_W);
        assert_eq!(up.y, 60.0);
        assert_eq!(down.x, up.x);
        assert!(down.y > up.y, "the rocker's halves must not overlap");
        assert!(
            row.increment.x + row.increment.w <= up.x,
            "a row's increment must stay clear of the scroll gutter"
        );
        assert_eq!(
            hit(rects, 0, down.center()),
            Some(DevParamsEvent::ScrollDown)
        );
        assert_eq!(hit(rects, 0, FALLBACK_LIST.center()), None);
    }

    /// A missing layout file leaves every rect on the decoded fallback.
    #[test]
    fn an_absent_layout_file_falls_back_to_the_decoded_rects() {
        assert_eq!(PanelRects::from_layout(None), PanelRects::default());
    }

    #[test]
    fn done_activates_as_done_and_steps_do_not() {
        // `activate` on the step events is deliberately not exercised here:
        // it mutates the process-global registry, which parallel tests read
        // (the same rule dev_params' own tests follow). The step math is
        // `get + step` through `set`'s tested clamp/snap; that a click
        // really moves a value is proven by the SDK e2e
        // (`dev-menu.e2e.test.ts`).
        let mut scroll = 0;
        assert!(activate(
            PanelRects::default(),
            DevParamsEvent::Done,
            &mut scroll
        ));
        assert_eq!(scroll, 0, "leaving the screen must not scroll it");
    }

    #[test]
    fn values_format_as_two_decimal_floats() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        assert_eq!(format_value(&kind, 0.72, None), "0.72");
        // The snap grid's f32 wobble (0.71999997) must not leak into the UI.
        assert_eq!(format_value(&kind, 0.719_999_97, None), "0.72");
        assert_eq!(format_value(&kind, 2.0, None), "2.00");
    }

    #[test]
    fn bools_format_as_on_and_off() {
        assert_eq!(format_value(&DevParamKind::Bool, 1.0, None), "On");
        assert_eq!(format_value(&DevParamKind::Bool, 0.0, None), "Off");
    }
}
