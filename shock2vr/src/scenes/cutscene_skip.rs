//! Hold-to-skip for the cutscene player: the hold latch and the radial progress
//! art it draws. Both are pure, so the timing rule and the ring are testable
//! without a running game.

use std::time::Duration;

use engine::texture_format::{PixelFormat, RawTextureData};

/// How long either trigger must be held to skip. Long enough that a trigger
/// brushed on the way into a cutscene does not dismiss it.
pub const SKIP_HOLD_DURATION: Duration = Duration::from_millis(1500);

/// Analog triggers rest a little above zero, so a resting hand must not read as
/// a hold - nor light up the affordance. Deliberately below the shared
/// press threshold ([`crate::ui::VR_TRIGGER_THRESHOLD`]): this is "the trigger
/// is being touched", which is when the affordance should appear, not "the
/// trigger is pressed".
const TRIGGER_TOUCH: f32 = 0.2;

/// How fast the affordance fades in and back out, in alpha per second.
const FADE_RATE: f32 = 4.0;

/// The trigger hold that skips a cutscene: it accumulates while either trigger
/// is held past [`TRIGGER_TOUCH`] and resets the moment it is released.
#[derive(Default)]
pub struct SkipHold {
    held: Duration,
    /// Opacity of the affordance, so releasing fades it out rather than
    /// blinking it away.
    alpha: f32,
    /// The trigger has been seen released since the cutscene began. A cutscene
    /// is often started by a trigger - a frobbed panel, a menu click - and that
    /// press is frequently still down as the movie opens; counting it would
    /// dismiss the movie the player just started.
    armed: bool,
}

impl SkipHold {
    /// Advance the hold by one frame's `elapsed` and return whether the hold is
    /// now complete - i.e. the cutscene should skip.
    pub fn update(&mut self, trigger_value: f32, elapsed: Duration) -> bool {
        let touched = trigger_value >= TRIGGER_TOUCH;
        self.armed |= !touched;
        let touched = touched && self.armed;
        if touched {
            self.held = (self.held + elapsed).min(SKIP_HOLD_DURATION);
        } else {
            self.held = Duration::ZERO;
        }

        let target = if touched { 1.0 } else { 0.0 };
        let step = FADE_RATE * elapsed.as_secs_f32();
        self.alpha += (target - self.alpha).clamp(-step, step);

        self.is_complete()
    }

    pub fn is_complete(&self) -> bool {
        self.held >= SKIP_HOLD_DURATION
    }

    /// Hold progress, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        self.held.as_secs_f32() / SKIP_HOLD_DURATION.as_secs_f32()
    }

    /// Opacity of the affordance; 0.0 means it should not be drawn at all.
    pub fn alpha(&self) -> f32 {
        self.alpha
    }
}

/// Edge length of the generated ring texture. Small: the ring draws a few dozen
/// pixels across, so anything larger is only oversampling.
const RING_TEXTURE_SIZE: usize = 64;

/// Steps the fill is drawn in. The art changes only when the arc crosses one, so
/// a caller keyed on [`ring_step`] rebuilds and re-uploads the texture this many
/// times over a hold rather than once per frame.
pub const RING_STEPS: u32 = 48;

/// Which step `progress` falls in - the cache key for [`ring_texture`].
pub fn ring_step(progress: f32) -> u32 {
    (progress.clamp(0.0, 1.0) * RING_STEPS as f32).round() as u32
}

/// The ring art for a step from [`ring_step`].
pub fn ring_texture_for_step(step: u32) -> RawTextureData {
    ring_texture(step as f32 / RING_STEPS as f32)
}

/// Inner and outer radius of the ring, in units of half the texture edge.
const RING_INNER: f32 = 0.66;
const RING_OUTER: f32 = 0.86;
/// Alpha of the part of the ring the hold has not reached yet - present enough
/// to read as a track for the fill to run around.
const TRACK_ALPHA: u8 = 90;

/// The radial progress ring for a hold `progress` in 0.0..=1.0: a white arc
/// filling clockwise from twelve o'clock over a faint full-circle track,
/// transparent everywhere else.
pub fn ring_texture(progress: f32) -> RawTextureData {
    let progress = progress.clamp(0.0, 1.0);
    let size = RING_TEXTURE_SIZE;
    let half = size as f32 / 2.0;
    let mut bytes = Vec::with_capacity(size * size * 4);

    for y in 0..size {
        for x in 0..size {
            // Pixel centers, in -1..1 with +y up (the quad's v axis).
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = -(y as f32 + 0.5 - half) / half;
            let radius = (dx * dx + dy * dy).sqrt();

            let alpha = if !(RING_INNER..=RING_OUTER).contains(&radius) {
                0
            } else if angle_fraction(dx, dy) <= progress {
                255
            } else {
                TRACK_ALPHA
            };

            bytes.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }

    RawTextureData {
        width: size as u32,
        height: size as u32,
        bytes,
        format: PixelFormat::RGBA,
    }
}

/// Where a point sits around the ring: 0.0 at twelve o'clock, rising clockwise
/// to 1.0 back at the top.
fn angle_fraction(dx: f32, dy: f32) -> f32 {
    let angle = dx.atan2(dy); // -PI..PI, zero pointing up
    let turns = angle / std::f32::consts::TAU;
    if turns < 0.0 { turns + 1.0 } else { turns }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(100);

    /// Release the trigger for a frame, which is what arms a fresh hold.
    fn armed() -> SkipHold {
        let mut hold = SkipHold::default();
        hold.update(0.0, FRAME);
        hold
    }

    /// The trigger that started the cutscene is often still down as it opens.
    #[test]
    fn a_trigger_already_held_at_the_start_does_not_skip() {
        let mut hold = SkipHold::default();
        assert!(!hold.update(1.0, SKIP_HOLD_DURATION * 3));
        assert_eq!(hold.progress(), 0.0);

        // Once released, a fresh hold counts as normal.
        hold.update(0.0, FRAME);
        assert!(hold.update(1.0, SKIP_HOLD_DURATION));
    }

    #[test]
    fn a_held_trigger_completes_only_after_the_hold_duration() {
        let mut hold = armed();
        let mut elapsed = Duration::ZERO;

        while elapsed + FRAME < SKIP_HOLD_DURATION {
            assert!(!hold.update(1.0, FRAME), "skipped early at {elapsed:?}");
            elapsed += FRAME;
        }

        assert!(hold.update(1.0, FRAME + FRAME));
        assert!(hold.is_complete());
        assert_eq!(hold.progress(), 1.0);
    }

    #[test]
    fn releasing_the_trigger_resets_the_hold() {
        let mut hold = armed();
        hold.update(1.0, SKIP_HOLD_DURATION - FRAME);
        assert!(hold.progress() > 0.9);

        hold.update(0.0, FRAME);
        assert_eq!(hold.progress(), 0.0);

        // A fresh hold must run the full duration again.
        assert!(!hold.update(1.0, SKIP_HOLD_DURATION - FRAME));
    }

    /// A resting analog trigger reads slightly above zero on real hardware.
    #[test]
    fn a_barely_touched_trigger_does_not_accumulate() {
        let mut hold = armed();
        hold.update(0.05, SKIP_HOLD_DURATION);
        assert_eq!(hold.progress(), 0.0);
        assert_eq!(hold.alpha(), 0.0);
    }

    #[test]
    fn the_affordance_fades_in_while_held_and_out_after_release() {
        let mut hold = armed();
        hold.update(1.0, FRAME);
        let faded_in = hold.alpha();
        assert!(faded_in > 0.0);

        hold.update(1.0, Duration::from_secs(1));
        assert_eq!(hold.alpha(), 1.0);

        hold.update(0.0, FRAME);
        assert!(hold.alpha() < 1.0 && hold.alpha() > 0.0);

        hold.update(0.0, Duration::from_secs(1));
        assert_eq!(hold.alpha(), 0.0);
    }

    /// Alpha of the ring pixel at the given angle in turns clockwise from the
    /// top, sampled mid-band.
    fn sample(texture: &RawTextureData, turns: f32) -> u8 {
        let size = texture.width as f32;
        let radius = (RING_INNER + RING_OUTER) / 2.0;
        let angle = turns * std::f32::consts::TAU;
        let x = (size / 2.0 + angle.sin() * radius * size / 2.0) as u32;
        let y = (size / 2.0 - angle.cos() * radius * size / 2.0) as u32;
        texture.bytes[((y * texture.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn the_ring_fills_clockwise_from_the_top() {
        let quarter = ring_texture(0.25);

        assert_eq!(sample(&quarter, 0.1), 255, "just past the top is filled");
        assert_eq!(sample(&quarter, 0.2), 255, "the first quarter is filled");
        assert_eq!(
            sample(&quarter, 0.4),
            TRACK_ALPHA,
            "past the fill only the track shows"
        );
        assert_eq!(sample(&quarter, 0.9), TRACK_ALPHA);

        let full = ring_texture(1.0);
        for turns in [0.1, 0.35, 0.6, 0.85] {
            assert_eq!(sample(&full, turns), 255, "a complete hold fills the ring");
        }
    }

    #[test]
    fn the_ring_is_transparent_off_the_band() {
        let texture = ring_texture(1.0);
        let center = ((texture.height / 2) * texture.width + texture.width / 2) * 4 + 3;
        assert_eq!(texture.bytes[center as usize], 0, "hollow center");
        assert_eq!(texture.bytes[3], 0, "transparent corner");
    }
}
