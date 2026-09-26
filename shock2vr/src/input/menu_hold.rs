//! What the left controller's Menu button means: a short press or a long one.
//!
//! The Touch has one Menu button to give (the right one belongs to the Quest
//! system UI) and two things to reach with it: jacking into the cyber
//! interface, which is a frequent in-game gesture, and the pause menu, which
//! is rare. So the button carries both - a short press jacks in, a hold of
//! [`MENU_LONG_PRESS`] pauses.
//!
//! The rule the machine below enforces, in order:
//!
//! * the short press fires on **release**, never on press, so a long press
//!   cannot also toggle the interface on its way past the threshold;
//! * the long press fires the moment the threshold is **crossed**, not on
//!   release, so the hold has a visible end (with the ring at
//!   [`crate::scenes::cutscene_skip`] filling up to it);
//! * the release that follows a long press is swallowed.
//!
//! Pure, so the whole timing rule is host-testable.

use std::time::Duration;

/// How long the Menu button must be held to reach the pause menu. Long enough
/// that jacking in never pauses by accident, short enough to sit out.
pub const MENU_LONG_PRESS: Duration = Duration::from_millis(500);

/// The most one frame can credit toward the hold. A frame longer than this is a
/// hitch (a level parse, a stall), not time the player spent on the button -
/// without the cap a tap landing on one would pause instantly.
const MAX_FRAME_CREDIT: Duration = Duration::from_millis(100);

/// What one Menu button press turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPress {
    /// Nothing decided this frame.
    None,
    /// Released before the threshold: jack in / out of the cyber interface.
    Short,
    /// Held past the threshold: open the pause menu.
    Long,
}

/// The Menu button's short/long press latch.
#[derive(Debug, Default)]
pub struct MenuHold {
    /// How long the button has been down, `None` while it is up.
    held: Option<Duration>,
    /// The long press already fired for this press, so the release is spent.
    fired_long: bool,
}

impl MenuHold {
    /// The button went down.
    pub fn press(&mut self) {
        self.held = Some(Duration::ZERO);
        self.fired_long = false;
    }

    /// Advance a held button by one frame, firing [`MenuPress::Long`] on the
    /// frame the threshold is crossed and nothing afterwards.
    pub fn tick(&mut self, elapsed: Duration) -> MenuPress {
        let Some(held) = self.held.as_mut() else {
            return MenuPress::None;
        };
        *held = held.saturating_add(elapsed.min(MAX_FRAME_CREDIT));
        if !self.fired_long && *held >= MENU_LONG_PRESS {
            self.fired_long = true;
            return MenuPress::Long;
        }
        MenuPress::None
    }

    /// The button came up: a short press unless the long one already fired.
    pub fn release(&mut self) -> MenuPress {
        let was_held = self.held.take().is_some();
        if self.fired_long || !was_held {
            self.fired_long = false;
            return MenuPress::None;
        }
        MenuPress::Short
    }

    /// Forget the press in flight without deciding anything: neither a short
    /// press nor a long one. For a button whose release can never arrive - an
    /// OpenXR session restart (a doff/don) makes the action inactive, so the
    /// runtime stops reporting edges for a button that may still be down - and
    /// for a scene where the hold means nothing. A plain `release` here would
    /// mint the short press nobody made.
    pub fn cancel(&mut self) {
        self.held = None;
        self.fired_long = false;
    }

    /// Hold progress toward the long press, `None` while the button is up or
    /// once the long press has fired - the readout is the *promise* of the
    /// pause menu, so it stops the moment the promise is kept.
    pub fn progress(&self) -> Option<f32> {
        if self.fired_long {
            return None;
        }
        let held = self.held?;
        Some((held.as_secs_f32() / MENU_LONG_PRESS.as_secs_f32()).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(100);

    /// Hold for `total`, a frame at a time (no frame may exceed
    /// [`MAX_FRAME_CREDIT`], so a test cannot hold for half a second in one
    /// tick any more than the game can). Returns the first press it decides.
    fn hold_for(hold: &mut MenuHold, total: Duration) -> MenuPress {
        let mut elapsed = Duration::ZERO;
        let mut decided = MenuPress::None;
        while elapsed < total {
            let step = FRAME.min(total - elapsed);
            let press = hold.tick(step);
            if decided == MenuPress::None {
                decided = press;
            }
            elapsed += step;
        }
        decided
    }

    #[test]
    fn a_quick_press_is_short_and_fires_on_release() {
        let mut hold = MenuHold::default();
        hold.press();
        assert_eq!(hold.tick(FRAME), MenuPress::None, "nothing fires on press");
        assert_eq!(hold.release(), MenuPress::Short);
    }

    /// The defect a naive "toggle on press, pause on hold" would have: the
    /// interface would open on the way to every pause.
    #[test]
    fn a_long_press_pauses_and_never_also_toggles_the_interface() {
        let mut hold = MenuHold::default();
        hold.press();

        let mut elapsed = Duration::ZERO;
        while elapsed + FRAME < MENU_LONG_PRESS {
            assert_eq!(
                hold.tick(FRAME),
                MenuPress::None,
                "paused early at {elapsed:?}"
            );
            elapsed += FRAME;
        }
        assert_eq!(
            hold.tick(FRAME),
            MenuPress::Long,
            "the threshold must fire it"
        );

        // Held on past the threshold: it fires once, not every frame.
        assert_eq!(hold_for(&mut hold, MENU_LONG_PRESS * 4), MenuPress::None);
        // ...and the release it was already spent on is swallowed.
        assert_eq!(hold.release(), MenuPress::None);
    }

    #[test]
    fn a_release_just_under_the_threshold_is_still_short() {
        let mut hold = MenuHold::default();
        hold.press();
        assert_eq!(
            hold_for(&mut hold, MENU_LONG_PRESS - Duration::from_millis(1)),
            MenuPress::None
        );
        assert_eq!(hold.release(), MenuPress::Short);
    }

    /// A fresh press after a long one runs the full duration again, rather
    /// than inheriting the spent latch.
    #[test]
    fn a_press_after_a_long_one_starts_over() {
        let mut hold = MenuHold::default();
        hold.press();
        assert_eq!(hold_for(&mut hold, MENU_LONG_PRESS), MenuPress::Long);
        hold.release();

        hold.press();
        assert_eq!(hold.tick(FRAME), MenuPress::None);
        assert_eq!(hold.release(), MenuPress::Short);
    }

    /// Ticking or releasing a button nobody pressed decides nothing - a
    /// runtime that reports a stale release (focus lost, hand dropped) must
    /// not mint a press.
    #[test]
    fn an_unpressed_button_decides_nothing() {
        let mut hold = MenuHold::default();
        assert_eq!(hold_for(&mut hold, MENU_LONG_PRESS * 4), MenuPress::None);
        assert_eq!(hold.release(), MenuPress::None);
        assert_eq!(hold.progress(), None);
    }

    /// A press that spans a hitch (a level parse, a stall: wall `elapsed` in
    /// the hundreds of milliseconds) is still a tap, not a hold.
    #[test]
    fn one_enormous_frame_cannot_complete_a_hold() {
        let mut hold = MenuHold::default();
        hold.press();
        assert_eq!(hold.tick(Duration::from_secs(4)), MenuPress::None);
        assert_eq!(hold.release(), MenuPress::Short);
    }

    /// Cancelling decides nothing - the defect a plain `release` would have,
    /// which would mint a cyber-interface toggle nobody asked for.
    #[test]
    fn cancelling_a_press_in_flight_decides_nothing() {
        let mut hold = MenuHold::default();
        hold.press();
        hold.tick(FRAME);
        hold.cancel();
        assert_eq!(hold.progress(), None);
        assert_eq!(hold.release(), MenuPress::None);
        assert_eq!(hold_for(&mut hold, MENU_LONG_PRESS * 4), MenuPress::None);

        // ...and the next press is unaffected.
        hold.press();
        assert_eq!(hold_for(&mut hold, MENU_LONG_PRESS), MenuPress::Long);
    }

    #[test]
    fn progress_runs_to_one_then_stops_once_the_menu_is_promised() {
        let mut hold = MenuHold::default();
        assert_eq!(hold.progress(), None, "nothing to show while it is up");

        hold.press();
        assert_eq!(hold.progress(), Some(0.0));
        hold_for(&mut hold, MENU_LONG_PRESS / 2);
        assert_eq!(hold.progress(), Some(0.5));

        hold_for(&mut hold, MENU_LONG_PRESS);
        assert_eq!(hold.progress(), None, "the ring stops when the menu opens");
    }
}
