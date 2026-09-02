//! The shots a single trigger pull still owes.
//!
//! A gun's fire setting authors `burst` - shots per pull, `-1` meaning
//! unlimited while the trigger is down - and `burst_interval_ms`, the gap
//! between them: the pistol's BURST sends 3 rounds 10 ms apart, the assault
//! rifle's AUTO keeps firing every 100 ms until the trigger comes up. This is
//! the pure counting/timing half of that; `WeaponScript` fires the shots.

use dark::properties::GunSettingDesc;

/// What the burst wants done this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstStep {
    /// The next shot is not due yet.
    Wait,
    /// Fire one more shot now.
    Fire,
    /// The pull owes nothing more - drop the burst.
    Done,
}

/// The remainder of a burst: what is still owed, and when the next of it is
/// due. Created after the pull's own first shot.
#[derive(Debug)]
pub struct BurstState {
    /// Shots still owed, or `None` for an unlimited burst, which owes shots
    /// for as long as the trigger is held.
    remaining: Option<i32>,
    /// Seconds between shots inside the burst.
    interval: f32,
    /// Seconds until the next shot is due.
    time_to_next: f32,
    /// Whether the trigger is still down.
    held: bool,
    /// The burst was created this frame, by the pull's own shot. That shot's
    /// ammo is not spent until the frame's effects are applied, so the burst
    /// idles one frame rather than firing a second round against a magazine
    /// that still reads full.
    starting: bool,
}

impl BurstState {
    /// What a pull in this setting owes AFTER its first shot, or `None` when
    /// the setting sends a single shot per pull - which is every gun in the
    /// game bar the pistol's BURST and the assault rifle's AUTO.
    pub fn begin(setting: &GunSettingDesc) -> Option<BurstState> {
        // A negative count is the unlimited burst; one or fewer shots are
        // already paid out by the pull's own.
        let remaining = if setting.burst < 0 {
            None
        } else if setting.burst > 1 {
            Some(setting.burst - 1)
        } else {
            return None;
        };
        let interval = setting.burst_interval_ms as f32 / 1000.0;
        Some(BurstState {
            remaining,
            interval,
            time_to_next: interval,
            held: true,
            starting: true,
        })
    }

    /// The trigger came up. A finite burst plays out regardless - letting go of
    /// the pistol's BURST one frame in still sends all three rounds - so only
    /// an unlimited burst ends here.
    pub fn release(&mut self) {
        self.held = false;
    }

    /// Advance by `dt` seconds. `can_fire` is whether the gun could pay for
    /// another shot right now: a burst that runs the magazine dry simply
    /// stops, without a click. At most one shot comes out per call, so a burst
    /// interval shorter than a frame paces to the frame rate.
    pub fn advance(&mut self, dt: f32, can_fire: bool) -> BurstStep {
        // Ahead of the gates below, so the creation frame never judges the
        // burst on the stale ammo `starting` exists to sit out.
        if self.starting {
            self.starting = false;
            return BurstStep::Wait;
        }
        if !can_fire || self.remaining == Some(0) || (self.remaining.is_none() && !self.held) {
            return BurstStep::Done;
        }

        self.time_to_next -= dt;
        if self.time_to_next > 0.0 {
            return BurstStep::Wait;
        }
        // Carry the overshoot into the next gap so a long frame does not
        // stretch the burst, but never bank more than one shot's worth.
        self.time_to_next = (self.time_to_next + self.interval).max(0.0);
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining -= 1;
        }
        BurstStep::Fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 60 Hz simulation frame - the rate `advance` is called at.
    const FRAME: f32 = 1.0 / 60.0;

    /// The pistol as shipped: NORM sends one round a pull, BURST three 10 ms
    /// apart.
    fn pistol_burst() -> GunSettingDesc {
        GunSettingDesc {
            burst: 3,
            burst_interval_ms: 10,
            ..GunSettingDesc::default()
        }
    }

    /// The assault rifle's AUTO: unlimited while held, 100 ms apart.
    fn assault_rifle_auto() -> GunSettingDesc {
        GunSettingDesc {
            burst: -1,
            burst_interval_ms: 100,
            ..GunSettingDesc::default()
        }
    }

    /// Run `frames` frames, returning how many shots the burst asked for and
    /// whether it finished.
    fn run(state: &mut BurstState, frames: u32, can_fire: bool) -> (u32, bool) {
        let mut shots = 0;
        for _ in 0..frames {
            match state.advance(FRAME, can_fire) {
                BurstStep::Fire => shots += 1,
                BurstStep::Wait => {}
                BurstStep::Done => return (shots, true),
            }
        }
        (shots, false)
    }

    #[test]
    fn a_single_shot_setting_owes_nothing_after_its_pull() {
        assert!(BurstState::begin(&GunSettingDesc::default()).is_none());
        // Zero and one are the same thing: the pull's own shot is the whole of it.
        assert!(
            BurstState::begin(&GunSettingDesc {
                burst: 0,
                ..GunSettingDesc::default()
            })
            .is_none()
        );
    }

    #[test]
    fn the_pistols_burst_owes_two_more_shots_then_stops() {
        let mut state = BurstState::begin(&pistol_burst()).expect("BURST is a burst");
        let (shots, done) = run(&mut state, 60, true);
        assert_eq!(shots, 2, "three rounds a pull, one of them already fired");
        assert!(done, "and then the burst is over");
    }

    #[test]
    fn a_burst_fires_at_most_one_shot_a_frame() {
        // 10 ms is shorter than a frame, so the three-round burst still takes
        // one frame per round rather than emptying into a single update - and
        // its first round waits out the frame the pull fired in.
        let mut state = BurstState::begin(&pistol_burst()).unwrap();
        assert_eq!(state.advance(FRAME, true), BurstStep::Wait);
        assert_eq!(state.advance(FRAME, true), BurstStep::Fire);
        assert_eq!(state.advance(FRAME, true), BurstStep::Fire);
        assert_eq!(state.advance(FRAME, true), BurstStep::Done);
    }

    #[test]
    fn a_burst_never_fires_in_the_frame_the_pull_did() {
        // A zero-interval burst is the pushiest case: its next round is due
        // immediately, and it still has to wait out the pull's own frame.
        let mut state = BurstState::begin(&GunSettingDesc {
            burst: 3,
            burst_interval_ms: 0,
            ..GunSettingDesc::default()
        })
        .unwrap();
        assert_eq!(state.advance(FRAME, true), BurstStep::Wait);
        assert_eq!(state.advance(FRAME, true), BurstStep::Fire);
    }

    #[test]
    fn a_finite_burst_finishes_after_the_trigger_is_released() {
        let mut state = BurstState::begin(&pistol_burst()).unwrap();
        state.release();
        let (shots, done) = run(&mut state, 60, true);
        assert_eq!(shots, 2, "the remaining rounds go out anyway");
        assert!(done);
    }

    #[test]
    fn full_auto_keeps_firing_while_the_trigger_is_down() {
        let mut state = BurstState::begin(&assault_rifle_auto()).unwrap();
        // A second of holding, at 100 ms a shot: the pull's own shot plus these.
        let (shots, done) = run(&mut state, 60, true);
        assert!(
            (8..=10).contains(&shots),
            "a second of AUTO is about nine more rounds, got {shots}"
        );
        assert!(!done, "and it is still going");
    }

    #[test]
    fn full_auto_stops_when_the_trigger_comes_up() {
        let mut state = BurstState::begin(&assault_rifle_auto()).unwrap();
        run(&mut state, 30, true);
        state.release();
        let (shots, done) = run(&mut state, 60, true);
        assert_eq!(shots, 0, "nothing more goes out after the release");
        assert!(done);
    }

    #[test]
    fn a_burst_stops_when_the_magazine_cannot_pay_for_the_next_shot() {
        let mut state = BurstState::begin(&pistol_burst()).unwrap();
        assert_eq!(state.advance(FRAME, true), BurstStep::Wait);
        assert_eq!(state.advance(FRAME, true), BurstStep::Fire);
        assert_eq!(state.advance(FRAME, false), BurstStep::Done);
    }
}
