use std::time::Duration;

use shipyard::Unique;

#[derive(Unique, Clone, Debug, Default)]
pub struct Time {
    pub elapsed: Duration,
    pub total: Duration,
}

/// The player's unscaled tick, published beside the world [`Time`] for
/// player UI and held-item code that reads time from the ECS.
#[derive(Unique, Clone, Debug, Default)]
pub struct PlayerTime(pub Time);

/// Derives world time from the player's gameplay tick, so the world can run
/// slower than the player (`world_time_scale`). Accumulates the time the world
/// skipped rather than multiplying totals, so changing the scale never moves
/// world "now" backwards or jumps deadlines.
#[derive(Clone, Debug, Default)]
pub struct WorldClock {
    lost: Duration,
}

impl WorldClock {
    pub fn advance(&mut self, tick: &Time, scale: f32) -> Time {
        if scale >= 1.0 {
            return Time {
                elapsed: tick.elapsed,
                total: tick.total.saturating_sub(self.lost),
            };
        }
        let elapsed = tick.elapsed.mul_f32(scale.max(0.0));
        self.lost += tick.elapsed.saturating_sub(elapsed);
        Time {
            elapsed,
            total: tick.total.saturating_sub(self.lost),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_nanos(16_666_667);

    fn run(clock: &mut WorldClock, tick: &mut Time, scale: f32, frames: u32) -> Time {
        let mut world = Time::default();
        for _ in 0..frames {
            tick.elapsed = FRAME;
            tick.total += FRAME;
            world = clock.advance(tick, scale);
        }
        world
    }

    #[test]
    fn identity_at_full_scale() {
        let mut clock = WorldClock::default();
        let tick = Time {
            elapsed: FRAME,
            total: FRAME * 7,
        };
        let world = clock.advance(&tick, 1.0);
        assert_eq!(world.elapsed, tick.elapsed);
        assert_eq!(world.total, tick.total);
    }

    #[test]
    fn half_scale_total_matches_half_the_ticks() {
        let mut slow = WorldClock::default();
        let mut slow_tick = Time::default();
        let slowed = run(&mut slow, &mut slow_tick, 0.5, 120);

        let mut full = WorldClock::default();
        let mut full_tick = Time::default();
        let baseline = run(&mut full, &mut full_tick, 1.0, 60);

        let diff = slowed.total.abs_diff(baseline.total);
        assert!(diff < Duration::from_micros(10), "{diff:?}");
    }

    #[test]
    fn total_is_monotonic_across_scale_changes() {
        let mut clock = WorldClock::default();
        let mut tick = Time::default();
        let mut last = Duration::ZERO;
        for scale in [1.0, 0.25, 1.0] {
            for _ in 0..30 {
                tick.elapsed = FRAME;
                tick.total += FRAME;
                let world = clock.advance(&tick, scale);
                assert!(world.total >= last);
                last = world.total;
            }
        }
        // 30 + 7.5 + 30 frames of world time.
        let expected = FRAME.mul_f32(67.5);
        assert!(
            last.abs_diff(expected) < Duration::from_micros(10),
            "{last:?}"
        );
    }

    #[test]
    fn zero_dt_tick_advances_nothing() {
        let mut clock = WorldClock::default();
        let mut tick = Time::default();
        let before = run(&mut clock, &mut tick, 0.5, 10);
        tick.elapsed = Duration::ZERO;
        let after = clock.advance(&tick, 0.5);
        assert_eq!(after.elapsed, Duration::ZERO);
        assert_eq!(after.total, before.total);
    }
}
