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
//! [`crate::scenes::LoadGameScene`]). Rows live inside the pane; "Done" sits
//! on the button art.
//!
//! [`DeveloperScene`]: crate::scenes::DeveloperScene

use cgmath::{Vector2, vec2};
use dark::{importers::UI_LAYOUT_IMPORTER, map::MapRect};
use engine::assets::asset_cache::AssetCache;

use super::{HAlign, Rect, UiCanvas, VAlign};
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

/// Indices into `GAMELODR.BIN`: header line, the dark list pane, (2 is the
/// load screen's "Load" button, unused here) and the bottom-right button art.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const DONE_RECT_INDEX: usize = 3;

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent - the
/// same fallbacks the load screen carries for the same art.
const FALLBACK_HEADER: Rect = Rect::new(261.0, 31.0, 202.0, 20.0);
const FALLBACK_LIST: Rect = Rect::new(261.0, 54.0, 202.0, 290.0);
const FALLBACK_DONE: Rect = Rect::new(527.0, 405.0, 95.0, 62.0);

/// Canvas y where the backdrop paints its bordered name-entry field; rows
/// stop above it (see the sibling constant on the load screen).
const FIELD_TOP_Y: f32 = 323.0;

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
    done: Rect,
}

impl Default for PanelRects {
    fn default() -> Self {
        Self {
            header: FALLBACK_HEADER,
            list: FALLBACK_LIST,
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
            done: at(DONE_RECT_INDEX, FALLBACK_DONE),
        }
    }

    /// Canvas center of the "Done" button, for hosts that need to reason
    /// about where it lands relative to their own widgets (the pause overlay
    /// asserts that its "Quit" button sits under it).
    pub fn done_center(&self) -> Vector2<f32> {
        self.done.center()
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
/// Tightened from 28/24 when the registry reached ten parameters and the
/// tenth stopped fitting - which `every_registered_param_fits_in_the_pane`
/// caught, and which would otherwise have silently hidden the newest knob
/// (`melee_volumes`) from the screen that exists to reach it. At this pitch
/// the pane holds exactly ten with nothing to spare, so the *next* parameter
/// needs the list to scroll rather than another shave; the assertion is what
/// will say so.
const ROW_PITCH: f32 = 25.0;
const ROW_H: f32 = 22.0;
/// Horizontal inset from the pane's edges, matching the load list's text
/// inset so the two screens' contents align inside the same art.
const TEXT_INSET: f32 = 8.0;
/// Width of the `<` / `>` hit regions.
const ARROW_W: f32 = 20.0;
/// Width of the value readout between the arrows.
const VALUE_W: f32 = 48.0;

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
    let right = list.x + list.w - TEXT_INSET;
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

/// How many rows fit in the pane above the backdrop's painted field. The
/// registry is expected to stay well under this; the cap only keeps a grown
/// table from drawing rows through the art.
fn visible_row_count(rects: PanelRects) -> usize {
    let list = rects.list;
    let usable = (list.y + list.h).min(FIELD_TOP_Y) - list.y;
    ((usable / ROW_PITCH).floor().max(0.0) as usize).min(dev_params::PARAMS.len())
}

/// The value readout: floats as `{:.2}`, the format the step grids are
/// declared in.
fn format_value(kind: &DevParamKind, value: f32) -> String {
    match kind {
        DevParamKind::Float { .. } => format!("{value:.2}"),
    }
}

/// The event at a canvas point, if any. Shared by the click and the hover
/// highlight, so the two can never disagree about where a button is.
pub fn hit(rects: PanelRects, point: Vector2<f32>) -> Option<DevParamsEvent> {
    let mut canvas = UiCanvas::<DevParamsEvent>::with_events(vec2(CANVAS_W, CANVAS_H));
    for (index, (id, _)) in dev_params::all().take(visible_row_count(rects)).enumerate() {
        let row = row_rects(rects, index);
        canvas.button(row.decrement, "", DevParamsEvent::Decrement(id));
        canvas.button(row.increment, "", DevParamsEvent::Increment(id));
    }
    canvas.button(rects.done, "", DevParamsEvent::Done);
    canvas.click_at(point)
}

/// Describe the panel onto the host's canvas: header, one row per parameter,
/// and "Done". `pointer_canvas` is the hover position in canvas pixels,
/// whatever produced it - the mouse or a VR controller ray; the highlight
/// resolves through the very same [`hit`] the click does.
pub fn draw(canvas: &mut UiCanvas, rects: PanelRects, pointer_canvas: Option<Vector2<f32>>) {
    let hovered = pointer_canvas.and_then(|point| hit(rects, point));
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

    for (index, (id, param)) in dev_params::all().take(visible_row_count(rects)).enumerate() {
        let row = row_rects(rects, index);
        canvas
            .text_native(
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
                &format_value(&param.kind, dev_params::get(id)),
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

/// Apply a clicked event to the registry. Returns `true` when the event was
/// [`DevParamsEvent::Done`] - the one thing the host must act on (leave the
/// screen); the steps are absorbed here so both hosts stay a one-liner.
pub fn activate(event: DevParamsEvent) -> bool {
    let step_by = |id: DevParamId, direction: f32| {
        let DevParamKind::Float { step, .. } = dev_params::spec(id).kind;
        // `set` clamps into range and snaps to the step grid, so walking off
        // either end just pins to it.
        dev_params::set(id, dev_params::get(id) + direction * step);
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
        DevParamsEvent::Done => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param_ids() -> Vec<DevParamId> {
        dev_params::all().map(|(id, _)| id).collect()
    }

    /// Every registered parameter must actually fit on the screen - a row
    /// drawn below the cap would be visible art damage AND an unreachable
    /// control, so growth past the pane is a test failure, not a truncation.
    #[test]
    fn every_registered_param_fits_in_the_pane() {
        let rects = PanelRects::default();
        assert_eq!(visible_row_count(rects), dev_params::PARAMS.len());
        let last = row_rects(rects, dev_params::PARAMS.len() - 1);
        assert!(last.label.y + ROW_H <= FIELD_TOP_Y);
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
        for (index, id) in ids.iter().enumerate() {
            let row = row_rects(rects, index);
            assert_eq!(
                hit(rects, row.decrement.center()),
                Some(DevParamsEvent::Decrement(*id)),
                "row {index} <"
            );
            assert_eq!(
                hit(rects, row.increment.center()),
                Some(DevParamsEvent::Increment(*id)),
                "row {index} >"
            );
            // The label and the value are readouts, not buttons.
            assert_eq!(hit(rects, row.label.center()), None);
            assert_eq!(hit(rects, row.value.center()), None);
        }
        assert_eq!(hit(rects, rects.done.center()), Some(DevParamsEvent::Done));
        // Bare backdrop is not a control.
        assert_eq!(hit(rects, vec2(50.0, 50.0)), None);
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
                ul_x: 0,
                ul_y: 0,
                lr_x: 1,
                lr_y: 1,
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
        assert_eq!(hit(rects, rects.done.center()), Some(DevParamsEvent::Done));
        assert_eq!(hit(rects, FALLBACK_DONE.center()), None);
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
        assert!(activate(DevParamsEvent::Done));
    }

    #[test]
    fn values_format_as_two_decimal_floats() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        assert_eq!(format_value(&kind, 0.72), "0.72");
        // The snap grid's f32 wobble (0.71999997) must not leak into the UI.
        assert_eq!(format_value(&kind, 0.719_999_97), "0.72");
        assert_eq!(format_value(&kind, 2.0), "2.00");
    }
}
