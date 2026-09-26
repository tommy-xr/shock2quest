//! The HUD status-message line: short lines of text the game tells the player
//! ("This lift has been taken offline for repairs.").
//!
//! The original draws these as a stack of up to six lines under the top-left
//! of the HUD, each expiring five seconds after it was added; a seventh line
//! scrolls the oldest one off. Placement lives here once, in 640x480 canvas
//! pixels: flat emits it into the HUD canvas, VR draws the same canvas on a
//! head-anchored panel.

use std::time::Duration;

use cgmath::{Vector2, vec2};
use shipyard::Unique;

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// How long a line stays up. The original's default message time (5 s).
pub const MESSAGE_DURATION: Duration = Duration::from_secs(5);

/// How many lines are shown at once; a further line scrolls the oldest off.
pub const MAX_LINES: usize = 6;

/// The block's upper-left corner on the 640x480 HUD canvas, and the width the
/// original wraps its message text within.
const ORIGIN: Vector2<f32> = vec2(192.0, 18.0);
const LINE_WIDTH: f32 = 446.0;
/// One line's box: the bitmap font's native height plus the original's 5px
/// inter-line spacing.
const LINE_HEIGHT: f32 = 20.0;

const FONT: &str = "mainfont.fon";

/// The message block's own canvas size, used by the VR panel.
pub const PANEL_SIZE: Vector2<f32> = vec2(LINE_WIDTH, LINE_HEIGHT * MAX_LINES as f32);

/// The status messages currently on screen, oldest first, each with the
/// mission time it stops being shown.
#[derive(Unique, Default)]
pub struct HudMessages {
    lines: Vec<(Duration, String)>,
}

impl HudMessages {
    /// Show `text` for [`MESSAGE_DURATION`]. Past [`MAX_LINES`] the oldest line
    /// scrolls off, as the original's overlay does.
    pub fn push(&mut self, text: String, now: Duration) {
        self.lines.retain(|(expiry, _)| *expiry > now);
        if self.lines.len() == MAX_LINES {
            self.lines.remove(0);
        }
        self.lines.push((now + MESSAGE_DURATION, text));
    }

    /// The lines still showing at `now`, oldest first. Expired lines are
    /// dropped here rather than on a timer (the `DamageFlash` pattern).
    pub fn active(&mut self, now: Duration) -> Vec<String> {
        self.lines.retain(|(expiry, _)| *expiry > now);
        self.lines.iter().map(|(_, text)| text.clone()).collect()
    }
}

/// Emit the message lines into `canvas` with the block's upper-left corner at
/// `origin` in that canvas's pixel space.
pub(crate) fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, lines: &[String]) {
    for (index, line) in lines.iter().take(MAX_LINES).enumerate() {
        canvas.text_native(
            Rect::new(
                origin.x,
                origin.y + LINE_HEIGHT * index as f32,
                LINE_WIDTH,
                LINE_HEIGHT,
            ),
            line,
            FONT,
            HAlign::Left,
            VAlign::Top,
        );
    }
}

/// Where the block sits on the flat HUD canvas.
pub(crate) fn flat_origin() -> Vector2<f32> {
    ORIGIN
}

/// The block as the VR head panel draws it: the same lines on a canvas that is
/// exactly the block, so the panel *is* the block (the `ammo_panel` pattern).
pub(crate) fn build_message_canvas(lines: &[String]) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL_SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), lines);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_message_shows_for_five_seconds() {
        let mut messages = HudMessages::default();
        messages.push("Access Denied.".to_string(), secs(10));

        assert_eq!(messages.active(secs(14)), vec!["Access Denied."]);
        assert!(messages.active(secs(15)).is_empty());
    }

    #[test]
    fn a_seventh_line_scrolls_the_oldest_off() {
        let mut messages = HudMessages::default();
        for index in 0..7 {
            messages.push(format!("line {index}"), secs(10));
        }

        let active = messages.active(secs(11));
        assert_eq!(active.len(), MAX_LINES);
        assert_eq!(active.first().unwrap(), "line 1");
        assert_eq!(active.last().unwrap(), "line 6");
    }

    /// Flat and VR emit the same lines at the same offsets; only the block's
    /// origin differs, so a line cannot land differently between them.
    #[test]
    fn the_panel_canvas_holds_one_element_per_line_stacked_by_line_height() {
        let canvas = build_message_canvas(&["one".to_string(), "two".to_string()]);

        let rects: Vec<Rect> = canvas
            .elements()
            .iter()
            .map(|element| element.rect())
            .collect();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].y, 0.0);
        assert_eq!(rects[1].y, LINE_HEIGHT);
    }
}
