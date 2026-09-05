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
/// given each clip's authored facing change and how long it runs.
///
/// `None` when no clip fits: a turn longer than `max_duration` keeps the
/// creature standing still for longer than it can afford, and standing still
/// leaves it closer to `delta` than a badly-matched turn would (small pivots
/// have no clip at all - the shortest stock human turn is ~102 degrees).
pub fn nearest_turn_clip(
    delta: Deg<f32>,
    clips: &[(Deg<f32>, f32)],
    max_duration: f32,
) -> Option<usize> {
    clips
        .iter()
        .enumerate()
        .filter(|(_, (end, duration))| is_turn_clip(*end) && *duration <= max_duration)
        // Both the residual and the baseline are minimal angles: an
        // about-face authored at -177 covers a wanted +175 (8 degrees short),
        // and reading that as 352 would reject it.
        .map(|(index, (end, _))| {
            (
                index,
                signed_end_direction(Deg(signed_end_direction(*end).0 - delta.0))
                    .0
                    .abs(),
            )
        })
        .filter(|(_, residual)| *residual < delta.0.abs())
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stock human `+stand` schema: idle, left, right, about-face, with
    /// the seconds each one runs for.
    const HUMAN_STAND: [(Deg<f32>, f32); 4] = [
        (Deg(0.0), 5.03),
        (Deg(258.15), 3.60),
        (Deg(102.58), 3.83),
        (Deg(191.95), 4.80),
    ];
    const NO_BUDGET_LIMIT: f32 = 1000.0;

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
        assert_eq!(
            nearest_turn_clip(Deg(95.0), &HUMAN_STAND, NO_BUDGET_LIMIT),
            Some(2)
        );
        assert_eq!(
            nearest_turn_clip(Deg(-95.0), &HUMAN_STAND, NO_BUDGET_LIMIT),
            Some(1)
        );
        assert_eq!(
            nearest_turn_clip(Deg(-170.0), &HUMAN_STAND, NO_BUDGET_LIMIT),
            Some(3)
        );
    }

    #[test]
    fn a_small_pivot_has_no_clip_worth_playing() {
        assert_eq!(
            nearest_turn_clip(Deg(30.0), &HUMAN_STAND, NO_BUDGET_LIMIT),
            None
        );
        assert_eq!(
            nearest_turn_clip(Deg(-30.0), &HUMAN_STAND, NO_BUDGET_LIMIT),
            None
        );
    }

    #[test]
    fn a_schema_without_turn_clips_never_pivots() {
        assert_eq!(
            nearest_turn_clip(Deg(150.0), &[(Deg(0.0), 1.0)], NO_BUDGET_LIMIT),
            None
        );
        assert_eq!(nearest_turn_clip(Deg(150.0), &[], NO_BUDGET_LIMIT), None);
    }

    /// An about-face authored just the other side of 180 still covers an
    /// about-face wanted just this side of it.
    #[test]
    fn an_about_face_covers_a_pivot_across_the_half_turn() {
        // 182.97 reads as -177.03; a +175 pivot is 8 degrees away, not 352.
        let clips = [(Deg(182.97), 2.53), (Deg(112.39), 2.0)];
        assert_eq!(
            nearest_turn_clip(Deg(175.0), &clips, NO_BUDGET_LIMIT),
            Some(0)
        );
        assert_eq!(
            nearest_turn_clip(Deg(-175.0), &clips, NO_BUDGET_LIMIT),
            Some(0)
        );
    }

    /// A creature can only stand still for so long: an about-face it cannot
    /// afford is steered rather than performed.
    #[test]
    fn a_turn_the_creature_cannot_afford_is_not_picked() {
        assert_eq!(nearest_turn_clip(Deg(-170.0), &HUMAN_STAND, 4.0), Some(1));
        assert_eq!(nearest_turn_clip(Deg(-170.0), &HUMAN_STAND, 3.0), None);
    }
}
