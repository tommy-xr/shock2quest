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

use super::{HAlign, Rect, UiCanvas, VAlign};
use crate::dev_params::{self, DevParamId, DevParamKind};

/// The frontend screens are authored on the original 640x480 canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;

/// Display font for the header, the arrows and "Done" (`METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// Small data font for the row labels and value readouts (`mainfont.fon`).
const ROW_FONT: &str = "mainfont.fon";

/// `GAMELOD.PCX`'s header line (decoded `GAMELODR.BIN`, rect 0).
const HEADER_RECT: Rect = Rect::new(261.0, 31.0, 202.0, 20.0);
/// The backdrop's dark list pane the rows sit in (rect 1).
const LIST_RECT: Rect = Rect::new(261.0, 54.0, 202.0, 290.0);
/// The framed button art in the bottom-right corner (rect 3, the load
/// screen's "Done") - the same art hosts this screen's "Done".
const DONE_RECT: Rect = Rect::new(527.0, 405.0, 95.0, 62.0);
/// Canvas y where the backdrop paints its bordered name-entry field; rows
/// stop above it (see the sibling constant on the load screen).
const FIELD_TOP_Y: f32 = 323.0;

/// Vertical distance between row tops, and each row's own height. Taller
/// than the load list's 19px rows: these rows carry click targets (the
/// arrows), so they get more air and a bigger hit area.
const ROW_PITCH: f32 = 28.0;
const ROW_H: f32 = 24.0;
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

fn row_rects(index: usize) -> RowRects {
    let y = LIST_RECT.y + index as f32 * ROW_PITCH;
    let right = LIST_RECT.x + LIST_RECT.w - TEXT_INSET;
    let increment_x = right - ARROW_W;
    let value_x = increment_x - VALUE_W;
    let decrement_x = value_x - ARROW_W;
    let label_x = LIST_RECT.x + TEXT_INSET;
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
fn visible_row_count() -> usize {
    let usable = (LIST_RECT.y + LIST_RECT.h).min(FIELD_TOP_Y) - LIST_RECT.y;
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
pub fn hit(point: Vector2<f32>) -> Option<DevParamsEvent> {
    let mut canvas = UiCanvas::<DevParamsEvent>::with_events(vec2(CANVAS_W, CANVAS_H));
    for (index, (id, _)) in dev_params::all().take(visible_row_count()).enumerate() {
        let rects = row_rects(index);
        canvas.button(rects.decrement, "", DevParamsEvent::Decrement(id));
        canvas.button(rects.increment, "", DevParamsEvent::Increment(id));
    }
    canvas.button(DONE_RECT, "", DevParamsEvent::Done);
    canvas.click_at(point)
}

/// Describe the panel onto the host's canvas: header, one row per parameter,
/// and "Done". `pointer_canvas` is the hover position in canvas pixels,
/// whatever produced it - the mouse or a VR controller ray; the highlight
/// resolves through the very same [`hit`] the click does.
pub fn draw(canvas: &mut UiCanvas, pointer_canvas: Option<Vector2<f32>>) {
    let hovered = pointer_canvas.and_then(hit);
    let hover_opacity = |event: DevParamsEvent| {
        if hovered == Some(event) {
            HOVER_OPACITY
        } else {
            IDLE_OPACITY
        }
    };

    canvas.text_native(
        HEADER_RECT,
        "Developer",
        MENU_FONT,
        HAlign::Center,
        VAlign::Middle,
    );

    for (index, (id, param)) in dev_params::all().take(visible_row_count()).enumerate() {
        let rects = row_rects(index);
        canvas
            .text_native(
                rects.label,
                param.label,
                ROW_FONT,
                HAlign::Left,
                VAlign::Middle,
            )
            .opacity(READOUT_OPACITY);
        canvas
            .text_native(
                rects.decrement,
                "<",
                MENU_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(hover_opacity(DevParamsEvent::Decrement(id)));
        canvas
            .text_native(
                rects.value,
                &format_value(&param.kind, dev_params::get(id)),
                ROW_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(READOUT_OPACITY);
        canvas
            .text_native(
                rects.increment,
                ">",
                MENU_FONT,
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(hover_opacity(DevParamsEvent::Increment(id)));
    }

    canvas
        .text_native(DONE_RECT, "Done", MENU_FONT, HAlign::Center, VAlign::Middle)
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
        assert_eq!(visible_row_count(), dev_params::PARAMS.len());
        let last = row_rects(dev_params::PARAMS.len() - 1);
        assert!(last.label.y + ROW_H <= FIELD_TOP_Y);
    }

    #[test]
    fn rows_stay_inside_the_pane_horizontally() {
        let rects = row_rects(0);
        assert!(rects.label.x >= LIST_RECT.x);
        assert!(
            rects.increment.x + rects.increment.w <= LIST_RECT.x + LIST_RECT.w,
            "the increment arrow must not spill out of the pane art"
        );
        // Left to right: label, <, value, >, with no overlaps.
        assert!(rects.label.x + rects.label.w <= rects.decrement.x);
        assert!(rects.decrement.x + rects.decrement.w <= rects.value.x);
        assert!(rects.value.x + rects.value.w <= rects.increment.x);
    }

    #[test]
    fn the_arrows_and_done_hit_test() {
        let ids = param_ids();
        for (index, id) in ids.iter().enumerate() {
            let rects = row_rects(index);
            assert_eq!(
                hit(rects.decrement.center()),
                Some(DevParamsEvent::Decrement(*id)),
                "row {index} <"
            );
            assert_eq!(
                hit(rects.increment.center()),
                Some(DevParamsEvent::Increment(*id)),
                "row {index} >"
            );
            // The label and the value are readouts, not buttons.
            assert_eq!(hit(rects.label.center()), None);
            assert_eq!(hit(rects.value.center()), None);
        }
        assert_eq!(hit(DONE_RECT.center()), Some(DevParamsEvent::Done));
        // Bare backdrop is not a control.
        assert_eq!(hit(vec2(50.0, 50.0)), None);
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
