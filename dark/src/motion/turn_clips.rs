//! Picking the authored turn clip for a pivot.
//!
//! Creature schemas file their turn-in-place motions beside the idle stand
//! clip (human: humstand, humtul, humtur, humtu180); the only thing that
//! distinguishes them is the authored facing change each one ends on. So a
//! pivot picks the clip whose authored angle is nearest the heading it needs,
//! and plain standing picks from the clips that don't turn at all.

use cgmath::Deg;

/// Below this, a clip's authored facing change is incidental drift rather
/// than a turn (stock idle clips end a fraction of a degree off).
const TURN_CLIP_MIN_ANGLE: f32 = 20.0;

/// A clip's authored facing change as a signed minimal angle, in (-180, 180].
/// On disk it is an unsigned 0..360 heading.
pub fn signed_end_direction(end_direction: Deg<f32>) -> Deg<f32> {
    let wrapped = end_direction.0.rem_euclid(360.0);
    Deg(if wrapped > 180.0 {
        wrapped - 360.0
    } else {
        wrapped
    })
}

/// Whether a clip turns the creature, rather than leaving it facing the way
/// it started.
pub fn is_turn_clip(end_direction: Deg<f32>) -> bool {
    signed_end_direction(end_direction).0.abs() > TURN_CLIP_MIN_ANGLE
}

/// Index of the turn clip that best covers `delta` degrees of heading change,
/// or `None` when standing still leaves the creature closer to `delta` than
/// any authored turn would (small pivots have no clip: the shortest stock
/// human turn is ~102 degrees).
pub fn nearest_turn_clip(delta: Deg<f32>, end_directions: &[Deg<f32>]) -> Option<usize> {
    end_directions
        .iter()
        .enumerate()
        .filter(|(_, end)| is_turn_clip(**end))
        .map(|(index, end)| (index, (signed_end_direction(*end).0 - delta.0).abs()))
        .filter(|(_, residual)| *residual < delta.0.abs())
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stock human `+stand` schema: idle, left, right, about-face.
    const HUMAN_STAND: [Deg<f32>; 4] = [Deg(0.0), Deg(258.15), Deg(102.58), Deg(191.95)];

    #[test]
    fn an_unsigned_heading_reads_as_the_minimal_signed_turn() {
        assert_eq!(signed_end_direction(Deg(0.0)), Deg(0.0));
        assert_eq!(signed_end_direction(Deg(102.58)), Deg(102.58));
        assert!((signed_end_direction(Deg(258.15)).0 - -101.85).abs() < 1e-3);
        assert!((signed_end_direction(Deg(191.95)).0 - -168.05).abs() < 1e-3);
    }

    #[test]
    fn only_the_pivots_count_as_turn_clips() {
        assert!(!is_turn_clip(Deg(0.0)));
        assert!(!is_turn_clip(Deg(359.5)));
        assert!(is_turn_clip(Deg(102.58)));
        assert!(is_turn_clip(Deg(258.15)));
        assert!(is_turn_clip(Deg(191.95)));
    }

    #[test]
    fn a_pivot_takes_the_clip_nearest_its_heading_change() {
        assert_eq!(nearest_turn_clip(Deg(95.0), &HUMAN_STAND), Some(2));
        assert_eq!(nearest_turn_clip(Deg(-95.0), &HUMAN_STAND), Some(1));
        assert_eq!(nearest_turn_clip(Deg(-170.0), &HUMAN_STAND), Some(3));
    }

    #[test]
    fn a_small_pivot_has_no_clip_worth_playing() {
        assert_eq!(nearest_turn_clip(Deg(30.0), &HUMAN_STAND), None);
        assert_eq!(nearest_turn_clip(Deg(-30.0), &HUMAN_STAND), None);
    }

    #[test]
    fn a_schema_without_turn_clips_never_pivots() {
        assert_eq!(nearest_turn_clip(Deg(150.0), &[Deg(0.0)]), None);
        assert_eq!(nearest_turn_clip(Deg(150.0), &[]), None);
    }
}
