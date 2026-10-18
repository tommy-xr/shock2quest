//! Speed boosts. Retail's Speed Boost hypo doubles the player's speed for 20 s
//! (24 with Pharmo-Friendly). In the default Slow world mode (a Comfort
//! option) it halves the world's speed instead: the same speed relative to the
//! world, as bullet time, without artificially fast locomotion.
use std::time::Duration;

use crate::user_settings::SpeedBoostMode;

pub const HYPO_DURATION: Duration = Duration::from_secs(20);
/// Retail's player speed factor for the hypo.
const HYPO_SPEED: f32 = 2.0;

/// The world's time scale: the developer knob, slowed further by an active
/// boost in Slow world mode. The slower of the two wins; they do not multiply.
pub fn world_time_scale(dev_scale: f32, boost_active: bool, mode: SpeedBoostMode) -> f32 {
    if boost_active && mode == SpeedBoostMode::SlowWorld {
        dev_scale.min(1.0 / HYPO_SPEED)
    } else {
        dev_scale
    }
}

/// Multiplier on the player's stick speed: Classic boosts speed the player up.
pub fn player_speed_scale(boost_active: bool, mode: SpeedBoostMode) -> f32 {
    if boost_active && mode == SpeedBoostMode::Classic {
        HYPO_SPEED
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_world_halves_the_world_and_leaves_the_player() {
        let mode = SpeedBoostMode::SlowWorld;
        assert_eq!(world_time_scale(1.0, true, mode), 0.5);
        assert_eq!(player_speed_scale(true, mode), 1.0);
        // A slower developer knob wins rather than compounding.
        assert_eq!(world_time_scale(0.25, true, mode), 0.25);
        assert_eq!(world_time_scale(1.0, false, mode), 1.0);
    }

    #[test]
    fn classic_doubles_the_player_and_leaves_the_world() {
        let mode = SpeedBoostMode::Classic;
        assert_eq!(world_time_scale(1.0, true, mode), 1.0);
        assert_eq!(player_speed_scale(true, mode), 2.0);
        assert_eq!(player_speed_scale(false, mode), 1.0);
    }
}
