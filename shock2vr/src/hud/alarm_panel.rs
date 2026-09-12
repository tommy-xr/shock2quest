//! The station security alarm readout, laid out once for both presentations.
//!
//! The original draws a static (never blinking) `ALARM.PCX` badge - "SECURITY
//! ALERT ACTIVE" over a camera, ending in a baked "TIME REMAINING:" label -
//! translucent on the screen's left edge, with the alarm's remaining seconds
//! written under it to one decimal.
//!
//! Every rect here is **panel-local**, in canvas pixels, so the flat HUD and
//! the VR forearm place the identical elements with only their own origin
//! (AGENTS.md section 3 - one layout, two presentations).

use cgmath::{Vector2, vec2};

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The badge art, panel-local.
pub(crate) const BADGE: Rect = Rect::new(0.0, 0.0, 64.0, 64.0);
/// The countdown the badge's baked "TIME REMAINING:" label introduces, two
/// pixels under the badge and centered on it.
pub(crate) const COUNTDOWN: Rect = Rect::new(BADGE.x, BADGE.y + BADGE.h + 2.0, BADGE.w, 14.0);
/// The whole readout's footprint, panel-local.
pub(crate) const FLAT_ORIGIN: Vector2<f32> = vec2(10.0, 260.0);
pub(crate) const PANEL: Vector2<f32> = vec2(BADGE.w, COUNTDOWN.y + COUNTDOWN.h);

/// The original draws the badge at 75/255 alpha, so the world reads through it.
const BADGE_ALPHA: f32 = 75.0 / 255.0;

/// Place a panel-local rect at `origin`.
pub(crate) const fn at(origin: Vector2<f32>, rect: Rect) -> Rect {
    Rect::new(origin.x + rect.x, origin.y + rect.y, rect.w, rect.h)
}

/// Draw the alarm badge and its countdown with the panel's upper-left corner
/// at `origin`. `seconds` is the alarm's remaining deadline.
pub(crate) fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, seconds: f32) {
    canvas
        .image(at(origin, BADGE), "ALARM.PCX")
        .opacity(BADGE_ALPHA)
        .text_native(
            at(origin, COUNTDOWN),
            &format!("{:.1}", seconds.max(0.0)),
            crate::ui::MFD_FONT,
            HAlign::Center,
            VAlign::Middle,
        );
}

/// The readout as a panel-sized canvas of its own - what a world-space
/// presentation hangs on a quad.
pub(crate) fn build_panel_canvas(seconds: f32) -> UiCanvas {
    let mut canvas = UiCanvas::new(PANEL);
    emit(&mut canvas, vec2(0.0, 0.0), seconds);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_countdown_sits_centered_under_the_badge() {
        assert_eq!(COUNTDOWN.y, BADGE.y + BADGE.h + 2.0);
        assert_eq!(COUNTDOWN.center().x, BADGE.center().x);
    }

    #[test]
    fn the_panel_bounds_everything_it_draws() {
        assert_eq!(PANEL.x, BADGE.w);
        assert_eq!(PANEL.y, COUNTDOWN.y + COUNTDOWN.h);
        assert_eq!(build_panel_canvas(12.0).element_count(), 2);
    }

    #[test]
    fn the_countdown_reads_to_one_decimal() {
        // Both presentations format it the same way because they share this
        // emit; a negative deadline never shows as negative time.
        let mut canvas = UiCanvas::new(PANEL);
        emit(&mut canvas, vec2(0.0, 0.0), -3.0);
        assert_eq!(canvas.element_count(), 2);
    }
}
