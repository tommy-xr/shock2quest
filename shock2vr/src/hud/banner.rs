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
/// The plate is translucent so the card reads as an overlay on the scene
/// rather than a hole cut in it - dark enough to carry the text, light enough
/// to keep the room behind it visible.
const PLATE_OPACITY: f32 = 0.6;
/// How long the card ramps up when it appears and back down as it leaves.
///
/// The original's card pops (its script just schedules an add and a remove),
/// but a title card that appears instantly a foot from the player's eyes reads
/// as a flash in VR, so both presentations ease it - the same ramps, decided
/// once here. Either may be zero for an instant cut.
const FADE_IN: Duration = Duration::from_millis(300);
const FADE_OUT: Duration = Duration::from_millis(500);
/// The card's text is the game's own teal MFD face - the colour every other
/// in-world readout is drawn in, and the one the original's card uses.
const FONT: &str = crate::ui::MFD_FONT;

/// A banner as the HUD draws it this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveBanner {
    pub text: String,
    /// The fade ramp, 0..1. Multiplies everything the card draws.
    pub alpha: f32,
}

/// The banner showing right now, with the mission times it appeared and stops
/// being shown.
#[derive(Unique, Default)]
pub struct HudBanner {
    showing: Option<(Duration, Duration, String)>,
}

impl HudBanner {
    /// Show `text` for `duration`, replacing any banner already up. Lines are
    /// separated by `\n`.
    pub fn show(&mut self, text: String, now: Duration, duration: Duration) {
        self.showing = Some((now, now + duration, text));
    }

    /// The banner still showing at `now`, or `None`. An expired banner is
    /// dropped here rather than on a timer (the `HudMessages` pattern).
    pub fn active(&mut self, now: Duration) -> Option<ActiveBanner> {
        self.showing.take_if(|(_, expiry, _)| *expiry <= now);
        let (shown_at, expiry, text) = self.showing.as_ref()?;
        Some(ActiveBanner {
            text: text.clone(),
            alpha: fade_alpha(now.saturating_sub(*shown_at), expiry.saturating_sub(now)),
        })
    }
}

/// The card's opacity given how long it has been up and how long it has left.
///
/// The two ramps are independent ([`FADE_IN`], [`FADE_OUT`]) and the smaller
/// wins, so a card shorter than both simply never reaches full opacity instead
/// of snapping between the two curves. A zero-length ramp is an instant cut.
fn fade_alpha(elapsed: Duration, remaining: Duration) -> f32 {
    let ramp = |progress: Duration, over: Duration| {
        if over.is_zero() {
            return 1.0;
        }
        (progress.as_secs_f32() / over.as_secs_f32()).clamp(0.0, 1.0)
    };
    ramp(elapsed, FADE_IN).min(ramp(remaining, FADE_OUT))
}

/// The lines a banner draws: the shared string-table split (which handles the
/// two-character `\n` a `.STR` value writes its break as, and trims the
/// padding shipped strings use to fake centering), capped at [`MAX_LINES`].
fn lines(text: &str) -> Vec<&str> {
    let mut lines = crate::ui::label_lines(text);
    lines.truncate(MAX_LINES);
    lines
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
/// canvas's pixel space, at `alpha` of its fade ramp.
pub(crate) fn emit(canvas: &mut UiCanvas, center: Vector2<f32>, text: &str, alpha: f32) {
    let lines = lines(text);
    if lines.is_empty() {
        return;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let plate = plate(center, lines.len());
    canvas
        .fill(plate, PLATE_COLOR)
        .opacity(PLATE_OPACITY * alpha);
    for (index, line) in lines.iter().enumerate() {
        canvas
            .text_native(
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
            )
            .opacity(alpha);
    }
}

/// Where the plate's center sits on the flat HUD canvas.
pub(crate) fn flat_center() -> Vector2<f32> {
    CENTER
}

/// The plate's own canvas size for `text`, used by the VR panel.
pub(crate) fn panel_size(text: &str) -> Vector2<f32> {
    let rect = plate(vec2(0.0, 0.0), lines(text).len().max(1));
    vec2(rect.w, rect.h)
}

/// The banner as the VR head panel draws it: the same plate and lines on a
/// canvas that is exactly the plate, so the panel *is* the plate (the
/// `message_line` pattern).
pub(crate) fn build_banner_canvas(text: &str, alpha: f32) -> UiCanvas {
    let size = panel_size(text);
    let mut canvas = UiCanvas::new(size);
    emit(&mut canvas, size / 2.0, text, alpha);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    /// CHARGEN.STR `EarthText0`, verbatim: padded first line, and the break
    /// written as the two characters `\n`.
    const EARTH: &str = r"         4 Years Earlier\nRamsey Recruitment Ctr.";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn text_at(banner: &mut HudBanner, now: Duration) -> Option<String> {
        banner.active(now).map(|shown| shown.text)
    }

    #[test]
    fn a_banner_shows_for_its_duration_and_then_clears() {
        let mut banner = HudBanner::default();
        banner.show("Wave 1".to_string(), secs(10), secs(3));
        assert_eq!(text_at(&mut banner, secs(12)), Some("Wave 1".to_string()));
        assert_eq!(text_at(&mut banner, secs(13)), None);
        // Still gone once it has expired.
        assert_eq!(text_at(&mut banner, secs(13)), None);
    }

    #[test]
    fn a_second_banner_replaces_the_first() {
        let mut banner = HudBanner::default();
        banner.show("Wave 1".to_string(), secs(0), secs(7));
        banner.show("Wave 2".to_string(), secs(3), secs(7));
        assert_eq!(text_at(&mut banner, secs(4)), Some("Wave 2".to_string()));
    }

    /// The card ramps up when it appears, holds, and ramps back down - and the
    /// ramp is measured from when it was shown, not from the mission clock's
    /// zero, so a card shown an hour in still fades in.
    #[test]
    fn a_banner_fades_in_holds_and_fades_out() {
        let mut banner = HudBanner::default();
        let shown_at = secs(3600);
        banner.show("Wave 1".to_string(), shown_at, secs(7));

        let mut alpha = |at: Duration| banner_alpha(&mut banner, shown_at + at);
        assert_eq!(alpha(Duration::ZERO), 0.0);
        assert_eq!(alpha(FADE_IN / 2), 0.5);
        assert_eq!(alpha(FADE_IN), 1.0);
        assert_eq!(alpha(secs(3)), 1.0);
        assert_eq!(alpha(secs(7) - FADE_OUT), 1.0);
        assert_eq!(alpha(secs(7) - FADE_OUT / 2), 0.5);
        // The last frame before expiry is nearly gone, and after it nothing.
        assert!(alpha(secs(7) - Duration::from_millis(1)) < 0.01);
        assert_eq!(banner.active(shown_at + secs(7)), None);
    }

    /// A card shorter than its two ramps never reaches full opacity rather
    /// than snapping between them.
    #[test]
    fn a_card_shorter_than_its_ramps_peaks_below_full() {
        let mut banner = HudBanner::default();
        banner.show("Wave 1".to_string(), Duration::ZERO, FADE_IN);
        let peak = banner_alpha(&mut banner, FADE_IN / 2);
        assert!(peak > 0.0 && peak < 1.0, "{peak}");
    }

    fn banner_alpha(banner: &mut HudBanner, now: Duration) -> f32 {
        banner.active(now).expect("a banner").alpha
    }

    /// The shipped string pads its first line with spaces; centering it for
    /// real means those must not count as glyphs.
    #[test]
    fn the_shipped_earth_string_becomes_two_trimmed_lines() {
        assert_eq!(lines(EARTH), ["4 Years Earlier", "Ramsey Recruitment Ctr."]);
    }

    #[test]
    fn a_banner_is_a_plate_plus_one_text_element_per_line() {
        let canvas = build_banner_canvas(EARTH, 1.0);
        assert_eq!(canvas.element_count(), 3);
        assert!(matches!(canvas.elements()[0], UiElement::Fill { .. }));

        let one_line = build_banner_canvas("Wave 1", 1.0);
        assert_eq!(one_line.element_count(), 2);
    }

    /// The fade scales everything the card draws - the plate's own translucency
    /// included, so a half-faded card is half of 60%, not 60% of anything.
    #[test]
    fn the_fade_scales_the_plate_and_its_text_together() {
        let canvas = build_banner_canvas(EARTH, 0.5);
        let alphas: Vec<f32> = canvas
            .elements()
            .iter()
            .map(|element| match element {
                UiElement::Fill { alpha, .. } | UiElement::Text { alpha, .. } => *alpha,
                _ => panic!("unexpected element"),
            })
            .collect();
        assert_eq!(alphas, vec![PLATE_OPACITY * 0.5, 0.5, 0.5]);
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
        let canvas = build_banner_canvas(EARTH, 1.0);
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
        assert_eq!(build_banner_canvas("   \n  ", 1.0).element_count(), 0);
    }

    /// The VR panel hangs off the head panel, which spans the same 640x480
    /// canvas the flat HUD does, so its offset from that panel's centre is
    /// read straight off the shared layout - there is no second placement
    /// decision to drift (`render_vr_banner`).
    #[test]
    fn the_vr_offset_is_the_shared_layouts_own_distance_below_canvas_centre() {
        let canvas_centre_y = 480.0 / 2.0;
        assert_eq!(flat_center().y - canvas_centre_y, 89.0);
    }

    #[test]
    fn the_plate_is_horizontally_centered_on_the_hud_canvas() {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit(&mut canvas, flat_center(), EARTH, 1.0);
        let plate = canvas.elements()[0].rect();
        assert_eq!(plate.center().x, 320.0);
    }
}
