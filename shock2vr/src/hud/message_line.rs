//! The HUD status-message line: short lines of text the game tells the player
//! ("This lift has been taken offline for repairs.").
//!
//! The original draws up to six messages, wrapping each within 446 pixels.
//! Each expires five seconds after posting; a seventh message scrolls the
//! oldest whole message off. Placement lives here once, in 640x480 canvas
//! pixels: flat emits it into the HUD canvas, VR draws the same canvas on a
//! head-anchored panel.

use std::time::Duration;

use cgmath::{Vector2, vec2};
use shipyard::Unique;

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// How long a line stays up. The original's default message time (5 s).
pub const MESSAGE_DURATION: Duration = Duration::from_secs(5);

/// Maximum message entries (not wrapped lines), as in MAX_OVERLAY_LINES.
pub const MAX_LINES: usize = 6;

/// The block's upper-left corner on the 640x480 HUD canvas, and the width the
/// original wraps its message text within.
const ORIGIN: Vector2<f32> = vec2(192.0, 18.0);
const LINE_WIDTH: f32 = 446.0;
/// DrawOverlayText adds this after each whole message, not each wrapped row.
const MESSAGE_SPACING: f32 = 5.0;

pub(crate) const FONT: &str = crate::ui::MESSAGE_FONT;

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
pub(crate) fn emit(
    canvas: &mut UiCanvas,
    origin: Vector2<f32>,
    messages: &[String],
    font: &dyn engine::Font,
) -> f32 {
    let height = font.base_height();
    let width = LINE_WIDTH.min(canvas.size().x - origin.x);
    let mut y = origin.y;
    for message in messages
        .iter()
        .take(MAX_LINES)
        .filter(|text| !text.is_empty())
    {
        for line in engine::wrap_text_to_width(font, message, height, width) {
            canvas.text_native(
                Rect::new(origin.x, y, width, height),
                &line,
                FONT,
                HAlign::Left,
                VAlign::Top,
            );
            y += height;
        }
        y += MESSAGE_SPACING;
    }
    (y - origin.y - MESSAGE_SPACING).max(height)
}

/// DrawOverlayText's 640x480 positions. In use mode, keep clear of both the
/// inventory strip and the MFD's 252px canvas (including its side controls).
/// This is shared canvas placement, so flat and VR reserve the same space.
pub(crate) fn flat_origin(use_mode: bool) -> Vector2<f32> {
    if use_mode { vec2(258.0, 130.0) } else { ORIGIN }
}

/// The same message block, tightly sized for a world-space panel. Increasing
/// its wrapped height must not move the first line; the panel mount maps its
/// top-left back to flat_origin rather than centering it above the gaze.
pub(crate) fn build_message_canvas(messages: &[String], font: &dyn engine::Font) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(LINE_WIDTH, 1.0));
    let height = emit(&mut canvas, vec2(0.0, 0.0), messages, font);
    UiCanvas::from_elements(vec2(LINE_WIDTH, height), canvas.into_elements())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Native-height metrics without uploading a GL texture.
    pub(crate) struct StubFont;
    impl engine::Font for StubFont {
        fn get_texture(&self) -> std::rc::Rc<dyn engine::texture::TextureTrait> {
            unreachable!("layout only needs glyph metrics")
        }
        fn get_character_info(&self, _: char) -> Option<engine::FontCharacterInfo> {
            Some(engine::FontCharacterInfo {
                min_uv_x: 0.0,
                min_uv_y: 0.0,
                max_uv_x: 1.0,
                max_uv_y: 1.0,
                advance: 6.0,
            })
        }
        fn base_height(&self) -> f32 {
            11.0
        }
        fn get_half_pixel(&self) -> f32 {
            0.0
        }
    }

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

    #[test]
    fn one_post_with_two_visual_lines_keeps_one_lifetime() {
        let mut messages = HudMessages::default();
        messages.push(
            "Research completed!\nClick Reports to read it.".into(),
            secs(10),
        );
        assert_eq!(
            messages.active(secs(14)),
            vec!["Research completed!\nClick Reports to read it."]
        );
        assert!(messages.active(secs(15)).is_empty());
    }

    #[test]
    fn six_multiline_messages_retain_all_their_rows() {
        let mut messages = HudMessages::default();
        for index in 0..6 {
            messages.push(format!("message {index}\nsecond row"), secs(10));
        }
        let active = messages.active(secs(11));
        assert_eq!(active.len(), 6);
        let canvas = build_message_canvas(&active, &StubFont);
        assert_eq!(canvas.element_count(), 12);
        // Two native-height rows plus one 5px gap per message, no trailing gap.
        assert_eq!(canvas.size().y, 6.0 * (2.0 * 11.0 + 5.0) - 5.0);
        assert_eq!(active[0], "message 0\nsecond row");
    }

    #[test]
    fn wraps_at_original_width_and_spaces_between_messages_only() {
        let messages = [format!("{}last", "word ".repeat(20)), "next\nrow".into()];
        let canvas = build_message_canvas(&messages, &StubFont);
        let elements = canvas.elements();
        assert_eq!(elements.len(), 4);
        let rows: Vec<_> = elements.iter().map(|element| element.rect().y).collect();
        assert_eq!(rows, [0.0, 11.0, 27.0, 38.0]);
        for element in elements {
            let crate::ui::UiElement::Text {
                text,
                font,
                font_size,
                ..
            } = element
            else {
                panic!("expected message text");
            };
            assert_eq!(font, crate::ui::MESSAGE_FONT);
            assert_eq!(*font_size, 0.0, "use native font size");
            assert!(engine::measure_text_width(&StubFont, text, 11.0) <= 446.0);
        }
    }

    #[test]
    fn use_mode_messages_clear_side_controls_and_wrap_inside_the_canvas() {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit(
            &mut canvas,
            flat_origin(true),
            &["Installed laser pointer. ".repeat(8)],
            &StubFont,
        );
        assert!(canvas.element_count() > 1, "exercise wrapping");
        for element in canvas.elements() {
            let rect = element.rect();
            assert!(rect.x >= 2.0 + 252.0 + 4.0);
            assert!(rect.x + rect.w <= canvas.size().x);
            if let crate::ui::UiElement::Text { text, .. } = element {
                assert!(
                    engine::measure_text_width(
                        &StubFont,
                        text,
                        engine::Font::base_height(&StubFont)
                    ) <= rect.w
                );
            }
        }
    }

    /// Every wrapped glyph and line must retain its flat HUD pixel layout.
    #[test]
    fn flat_and_vr_message_blocks_have_identical_relative_layout() {
        let lines = ["word ".repeat(20), "two\nthree".to_string()];
        let mut flat = UiCanvas::new(vec2(640.0, 480.0));
        emit(&mut flat, flat_origin(false), &lines, &StubFont);
        let panel = build_message_canvas(&lines, &StubFont);

        let flat_rects: Vec<Rect> = flat
            .elements()
            .iter()
            .map(|element| element.rect())
            .collect();
        let panel_rects: Vec<Rect> = panel
            .elements()
            .iter()
            .map(|element| element.rect())
            .collect();
        assert_eq!(flat_rects.len(), panel_rects.len());
        for (flat, panel) in flat_rects.iter().zip(panel_rects.iter()) {
            assert_eq!(flat.x - flat_origin(false).x, panel.x);
            assert_eq!(flat.y - flat_origin(false).y, panel.y);
            assert_eq!(flat.w, panel.w);
            assert_eq!(flat.h, panel.h);
        }
        assert_eq!(panel_rects[1].y - panel_rects[0].y, 11.0);
    }
}
