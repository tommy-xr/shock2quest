//! The centered interstitial banner: a black plate carrying one or two lines
//! of centered text over the middle of the view.
//!
//! The original shows one of these at the start of the Earth mission ("4 Years
//! Earlier / Ramsey Recruitment Ctr.", `EarthText0` in CHARGEN.STR) - a title
//! card that appears just after the level loads and clears itself seven
//! seconds later. Placement lives here once, in 640x480 canvas pixels: flat
//! emits it into the HUD canvas, VR draws the same canvas on a head-anchored
//! panel.

use std::time::Duration;

use cgmath::{Vector2, vec2};
use shipyard::Unique;

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// At most two lines are shown; the plate is sized for them.
pub const MAX_LINES: usize = 2;

/// The plate's fixed width, and the vertical padding above and below the text.
const PLATE_WIDTH: f32 = 172.0;
const PAD_Y: f32 = 7.0;
/// One line's box: the bitmap font's native height plus inter-line spacing.
const LINE_HEIGHT: f32 = 20.0;

/// Where the plate's center sits on the 640x480 HUD canvas - horizontally
/// centered, a little below the crosshair, as the original's card is.
const CENTER: Vector2<f32> = vec2(320.0, 329.0);

const PLATE_COLOR: [u8; 3] = [0, 0, 0];
const FONT: &str = "mainfont.fon";

/// The banner showing right now, with the mission time it stops being shown.
#[derive(Unique, Default)]
pub struct HudBanner {
    showing: Option<(Duration, String)>,
}

impl HudBanner {
    /// Show `text` for `duration`, replacing any banner already up. Lines are
    /// separated by `\n`.
    pub fn show(&mut self, text: String, now: Duration, duration: Duration) {
        self.showing = Some((now + duration, text));
    }

    /// The banner still showing at `now`, or `None`. An expired banner is
    /// dropped here rather than on a timer (the `HudMessages` pattern).
    pub fn active(&mut self, now: Duration) -> Option<String> {
        self.showing.take_if(|(expiry, _)| *expiry <= now);
        self.showing.as_ref().map(|(_, text)| text.clone())
    }
}

/// The lines a banner draws: `\n`-separated, trimmed (the shipped string pads
/// its first line with spaces to fake centering, which we do for real), and
/// capped at [`MAX_LINES`].
fn lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .collect()
}

/// The plate's rect on a canvas, given the plate's center.
fn plate(center: Vector2<f32>, line_count: usize) -> Rect {
    let height = PAD_Y * 2.0 + LINE_HEIGHT * line_count as f32;
    Rect::new(
        center.x - PLATE_WIDTH / 2.0,
        center.y - height / 2.0,
        PLATE_WIDTH,
        height,
    )
}

/// Emit the banner into `canvas` with its plate centered on `center` in that
/// canvas's pixel space.
pub(crate) fn emit(canvas: &mut UiCanvas, center: Vector2<f32>, text: &str) {
    let lines = lines(text);
    if lines.is_empty() {
        return;
    }
    let plate = plate(center, lines.len());
    canvas.fill(plate, PLATE_COLOR);
    for (index, line) in lines.iter().enumerate() {
        canvas.text_native(
            Rect::new(
                plate.x,
                plate.y + PAD_Y + LINE_HEIGHT * index as f32,
                plate.w,
                LINE_HEIGHT,
            ),
            line,
            FONT,
            HAlign::Center,
            VAlign::Middle,
        );
    }
}

/// Where the plate's center sits on the flat HUD canvas.
pub(crate) fn flat_center() -> Vector2<f32> {
    CENTER
}

/// The plate's own canvas size for `text`, used by the VR panel.
pub(crate) fn panel_size(text: &str) -> Vector2<f32> {
    let rect = plate(CENTER, lines(text).len().max(1));
    vec2(rect.w, rect.h)
}

/// The banner as the VR head panel draws it: the same plate and lines on a
/// canvas that is exactly the plate, so the panel *is* the plate (the
/// `message_line` pattern).
pub(crate) fn build_banner_canvas(text: &str) -> UiCanvas {
    let size = panel_size(text);
    let mut canvas = UiCanvas::new(size);
    emit(&mut canvas, size / 2.0, text);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    const EARTH: &str = "         4 Years Earlier\nRamsey Recruitment Ctr.";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_banner_shows_for_its_duration_and_then_clears() {
        let mut banner = HudBanner::default();
        banner.show("Wave 1".to_string(), secs(10), secs(3));
        assert_eq!(banner.active(secs(12)), Some("Wave 1".to_string()));
        assert_eq!(banner.active(secs(13)), None);
        // Still gone once it has expired.
        assert_eq!(banner.active(secs(13)), None);
    }

    #[test]
    fn a_second_banner_replaces_the_first() {
        let mut banner = HudBanner::default();
        banner.show("Wave 1".to_string(), secs(0), secs(7));
        banner.show("Wave 2".to_string(), secs(3), secs(7));
        assert_eq!(banner.active(secs(4)), Some("Wave 2".to_string()));
    }

    /// The shipped string pads its first line with spaces; centering it for
    /// real means those must not count as glyphs.
    #[test]
    fn the_shipped_earth_string_becomes_two_trimmed_lines() {
        assert_eq!(lines(EARTH), ["4 Years Earlier", "Ramsey Recruitment Ctr."]);
    }

    #[test]
    fn a_banner_is_a_plate_plus_one_text_element_per_line() {
        let canvas = build_banner_canvas(EARTH);
        assert_eq!(canvas.element_count(), 3);
        assert!(matches!(canvas.elements()[0], UiElement::Fill { .. }));

        let one_line = build_banner_canvas("Wave 1");
        assert_eq!(one_line.element_count(), 2);
    }

    /// A one-line banner gets a shorter plate; the width never changes.
    #[test]
    fn the_plate_grows_by_a_line_height_per_line() {
        let one = panel_size("Wave 1");
        let two = panel_size(EARTH);
        assert_eq!(one.x, two.x);
        assert_eq!(two.y - one.y, LINE_HEIGHT);
    }

    /// Every line sits inside the plate, so no glyph spills onto bare 3D view.
    #[test]
    fn the_lines_sit_inside_the_plate() {
        let canvas = build_banner_canvas(EARTH);
        let plate = canvas.elements()[0].rect();
        for element in &canvas.elements()[1..] {
            let rect = element.rect();
            assert!(plate.x <= rect.x, "{rect:?} left of the plate");
            assert!(rect.x + rect.w <= plate.x + plate.w, "{rect:?} right of it");
            assert!(plate.y <= rect.y, "{rect:?} above the plate");
            assert!(rect.y + rect.h <= plate.y + plate.h, "{rect:?} below it");
        }
    }

    /// Empty or whitespace-only text draws nothing at all - not a bare plate.
    #[test]
    fn an_empty_banner_draws_nothing() {
        assert_eq!(build_banner_canvas("   \n  ").element_count(), 0);
    }

    #[test]
    fn the_plate_is_horizontally_centered_on_the_hud_canvas() {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit(&mut canvas, flat_center(), EARTH);
        let plate = canvas.elements()[0].rect();
        assert_eq!(plate.center().x, 320.0);
    }
}
