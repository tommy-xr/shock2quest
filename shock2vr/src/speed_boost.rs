//! Speed boosts. Retail's Speed Boost hypo doubles the player's speed for 20 s
//! (24 with Pharmo-Friendly). In the default Slow world mode (a Comfort
//! option) it halves the world's speed instead: the same speed relative to the
//! world, as bullet time, without artificially fast locomotion. Quickness
//! (Psychogenic Agility) becomes bullet time too in that mode, in place of its
//! Agility bonus, slowing the world further the higher the player's PSI.
use std::time::Duration;

use crate::user_settings::SpeedBoostMode;

pub const HYPO_DURATION: Duration = Duration::from_secs(20);
/// Retail's player speed factor for the hypo.
const HYPO_SPEED: f32 = 2.0;

/// Quickness's world scale at the player's PSI: 0.5 up to PSI 3, then 0.05
/// slower per point, reaching 0.25 at the PSI cap of 8.
pub fn quickness_world_scale(psi: i32) -> f32 {
    (0.5 - 0.05 * (psi - 3).max(0) as f32).max(0.25)
}

/// The world's time scale: the developer knob, slowed further by active
/// boosts in Slow world mode. The slowest wins; they do not multiply.
/// `quickness_psi` is the player's PSI while Quickness is active.
pub fn world_time_scale(
    dev_scale: f32,
    hypo_active: bool,
    quickness_psi: Option<i32>,
    mode: SpeedBoostMode,
) -> f32 {
    if mode == SpeedBoostMode::Classic {
        return dev_scale;
    }
    let hypo = if hypo_active { 1.0 / HYPO_SPEED } else { 1.0 };
    let quickness = quickness_psi.map_or(1.0, quickness_world_scale);
    dev_scale.min(hypo).min(quickness)
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
        assert_eq!(world_time_scale(1.0, true, None, mode), 0.5);
        assert_eq!(player_speed_scale(true, mode), 1.0);
        // A slower developer knob wins rather than compounding.
        assert_eq!(world_time_scale(0.25, true, None, mode), 0.25);
        assert_eq!(world_time_scale(1.0, false, None, mode), 1.0);
    }

    #[test]
    fn classic_doubles_the_player_and_leaves_the_world() {
        let mode = SpeedBoostMode::Classic;
        assert_eq!(world_time_scale(1.0, true, Some(8), mode), 1.0);
        assert_eq!(player_speed_scale(true, mode), 2.0);
        assert_eq!(player_speed_scale(false, mode), 1.0);
    }

    #[test]
    fn quickness_slows_the_world_more_with_psi() {
        let mode = SpeedBoostMode::SlowWorld;
        assert_eq!(world_time_scale(1.0, false, Some(1), mode), 0.5);
        assert_eq!(world_time_scale(1.0, false, Some(3), mode), 0.5);
        assert!((world_time_scale(1.0, false, Some(5), mode) - 0.4).abs() < 1e-6);
        assert_eq!(world_time_scale(1.0, false, Some(8), mode), 0.25);
        // With the hypo too, the slower boost wins.
        assert_eq!(world_time_scale(1.0, true, Some(8), mode), 0.25);
    }
}
