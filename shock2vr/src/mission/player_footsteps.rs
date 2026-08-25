//! Player footstep pacing.
//!
//! Creatures get their footsteps from authored animation flags
//! (`MotionFlags::LEFT_FOOT_STEP`, see `script_util::play_footstep_sound`).
//! The player has no locomotion animation, so theirs are derived from how far
//! they actually walked: a **horizontal distance accumulator**, not a timer.
//! Distance gives a slower cadence when moving slowly and a faster one when
//! running with no speed tiers, is frame-rate independent (Quest and desktop
//! run at different rates), and cannot tick while standing still.
//!
//! The accumulator is fed the player's *own* per-frame translation
//! (`PlayerHandle::self_translation`), which already has the moving-platform
//! carry removed, so a rider standing on an elevator accrues nothing. It is
//! also never fed a world-position delta, so a teleport hop - which relocates
//! the body without running a movement frame at all - cannot burst a run of
//! footsteps out of one frame.
//!
//! Known gap: room-scale VR walking never moves the capsule (headset
//! translation only places the camera), so physically stepping in the
//! playspace is silent. That is deliberate - the deck is not moving under the
//! player - rather than an oversight.

use cgmath::{InnerSpace, Vector3};
use dark::SCALE_FACTOR;
use engine::audio::AudioHandle;

use crate::scripts::Effect;

/// How far the player walks per footstep, in Dark units. Paired with
/// `PLAYER_MOVE_SPEED` (25 Dark units/second) this paces a full-speed walk at
/// roughly 3.5 steps/second - the same cadence the creature half measured for
/// a jogging hybrid.
const STRIDE_DISTANCE: f32 = 7.0 / SCALE_FACTOR;

/// Crouched stride multiplier. Crouching does not slow the player down in this
/// port, and the schema authors no quieter crouch sample (and volume is
/// authored per-sample, so it cannot be attenuated at the call site), so
/// spacing the steps further apart is the only lever that makes sneaking read
/// as quieter than walking.
const CROUCH_STRIDE_MULTIPLIER: f32 = 1.6;

/// How far the player must fall before touchdown plays the heavier
/// `landing=true` sample, in Dark units. Below this a landing is just the
/// player walking off a lip and is left to the ordinary stride.
const LANDING_MIN_FALL: f32 = 2.0 / SCALE_FACTOR;

/// Material tag every player footstep resolves under until world geometry
/// carries a per-texture material (`RayCastResult` has no surface id - see
/// `projects/footstep-sounds.md` §4). The ship is mostly metal bulkheads, so
/// this is right more often than not and wrong on carpet and soil.
const DEFAULT_GROUND_MATERIAL: &str = "metal";

/// What the accumulator decided this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerFootstep {
    /// An ordinary stride while walking.
    Step,
    /// Touchdown at the end of a meaningful fall (`landing=true`).
    Landing,
}

/// One frame of player locomotion, as the footstep pacer sees it.
#[derive(Clone, Copy, Debug)]
pub struct FootstepFrame {
    /// The player's own translation this frame, platform carry already
    /// removed (`PlayerHandle::self_translation`).
    pub self_translation: Vector3<f32>,
    pub is_grounded: bool,
    pub is_crouched: bool,
}

/// Distance-paced player footsteps. Purely derived per-frame state: a save
/// restores mid-stride at worst, which is inaudible.
#[derive(Debug)]
pub struct PlayerFootsteps {
    /// Horizontal distance walked since the last step, in world units.
    distance: f32,
    /// Downward distance travelled since leaving the ground, in world units.
    fall_distance: f32,
    was_grounded: bool,
}

impl PlayerFootsteps {
    pub fn new() -> Self {
        Self {
            distance: 0.0,
            fall_distance: 0.0,
            // Start grounded: a player who spawns standing on the deck must
            // not have their first movement frame read as a touchdown, which
            // would swallow the first stride.
            was_grounded: true,
        }
    }

    /// Forget the stride and the fall in progress. Called on any direct
    /// relocation (teleport locomotion, level load, quickload) so the player
    /// never arrives owing a footstep from wherever they were before.
    pub fn reset(&mut self) {
        self.distance = 0.0;
        self.fall_distance = 0.0;
        // Assume grounded so the arrival frame is not read as a landing.
        self.was_grounded = true;
    }

    /// Advance one movement frame, returning the footstep it produced (at most
    /// one per frame).
    pub fn update(&mut self, frame: FootstepFrame) -> Option<PlayerFootstep> {
        let was_grounded = self.was_grounded;
        self.was_grounded = frame.is_grounded;

        if !frame.is_grounded {
            // Airborne: a jump or a fall keeps accruing horizontal distance,
            // so the stride has to stop rather than play a step at the top of
            // the arc. Track the drop instead, for the landing below.
            if frame.self_translation.y < 0.0 {
                self.fall_distance -= frame.self_translation.y;
            }
            return None;
        }

        if !was_grounded {
            // Touchdown. Either way the stride restarts here, so a landing is
            // not immediately followed by a half-stride step.
            let fall_distance = self.fall_distance;
            self.fall_distance = 0.0;
            self.distance = 0.0;
            return (fall_distance >= LANDING_MIN_FALL).then_some(PlayerFootstep::Landing);
        }

        let stride = if frame.is_crouched {
            STRIDE_DISTANCE * CROUCH_STRIDE_MULTIPLIER
        } else {
            STRIDE_DISTANCE
        };
        let horizontal =
            cgmath::vec2(frame.self_translation.x, frame.self_translation.z).magnitude();
        self.distance += horizontal;
        if self.distance < stride {
            return None;
        }
        // Subtract rather than zero, so a frame long enough to cross a whole
        // stride does not lose the remainder and drift the cadence. One step
        // per frame is plenty - a single frame covering several strides is a
        // hitch, and a burst of footsteps is the wrong way to report one.
        self.distance = (self.distance - stride).min(stride);
        Some(PlayerFootstep::Step)
    }
}

/// The `Effect` for a player footstep at `position` (the pawn origin, which is
/// at the player's feet).
///
/// The player carries no `PropClassTag`, so `get_environmental_sound_query`
/// returns `None` for them and the schema query is built directly. The
/// landing variant asks for the most specific match: `landing=true` refines a
/// node that already resolves (`material=metal` -> `ftmet1..4`), and the
/// default shallowest-match resolution would hand back the ordinary footstep
/// and drop the thud.
pub fn player_footstep_effect(footstep: PlayerFootstep, position: Vector3<f32>) -> Effect {
    let mut tags = vec![
        ("event", "footstep"),
        ("creaturetype", "player"),
        ("material", DEFAULT_GROUND_MATERIAL),
    ];
    if footstep == PlayerFootstep::Landing {
        tags.push(("landing", "true"));
    }
    let query = dark::EnvSoundQuery::from_tag_values(tags);
    let query = if footstep == PlayerFootstep::Landing {
        query.most_specific()
    } else {
        query
    };

    Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query,
        position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;

    fn walking(distance: f32) -> FootstepFrame {
        FootstepFrame {
            self_translation: vec3(0.0, 0.0, distance),
            is_grounded: true,
            is_crouched: false,
        }
    }

    /// Run `frames` movement frames of `per_frame` forward travel, counting
    /// the footsteps they produced.
    fn count_steps(footsteps: &mut PlayerFootsteps, frames: usize, per_frame: f32) -> usize {
        (0..frames)
            .filter(|_| footsteps.update(walking(per_frame)) == Some(PlayerFootstep::Step))
            .count()
    }

    #[test]
    fn walking_a_stride_plays_one_footstep() {
        // Slightly over a stride, so the count does not hinge on a sum of
        // tenths landing exactly on it.
        let mut footsteps = PlayerFootsteps::new();
        let steps = count_steps(&mut footsteps, 11, STRIDE_DISTANCE / 10.0);
        assert_eq!(steps, 1);
    }

    #[test]
    fn footstep_cadence_scales_with_distance_not_frames() {
        // Ten strides in ten frames and in a thousand frames both produce ten
        // footsteps: the pacer is distance-driven, not per-frame.
        let mut coarse = PlayerFootsteps::new();
        assert_eq!(count_steps(&mut coarse, 10, STRIDE_DISTANCE), 10);

        let mut fine = PlayerFootsteps::new();
        assert_eq!(count_steps(&mut fine, 1005, STRIDE_DISTANCE / 100.0), 10);
    }

    #[test]
    fn standing_still_is_silent() {
        let mut footsteps = PlayerFootsteps::new();
        assert_eq!(count_steps(&mut footsteps, 600, 0.0), 0);
    }

    #[test]
    fn a_player_carried_by_an_elevator_is_silent() {
        // A rider standing still on a moving platform has a zero SELF
        // translation, however far the world moves them.
        let mut footsteps = PlayerFootsteps::new();
        let carried = FootstepFrame {
            self_translation: vec3(0.0, 0.0, 0.0),
            is_grounded: true,
            is_crouched: false,
        };
        assert!((0..600).all(|_| footsteps.update(carried).is_none()));
    }

    #[test]
    fn airborne_travel_plays_no_footstep() {
        let mut footsteps = PlayerFootsteps::new();
        let jumping = FootstepFrame {
            self_translation: vec3(0.0, 0.1, STRIDE_DISTANCE),
            is_grounded: false,
            is_crouched: false,
        };
        assert!((0..60).all(|_| footsteps.update(jumping).is_none()));
    }

    #[test]
    fn landing_after_a_fall_fires_once() {
        let mut footsteps = PlayerFootsteps::new();
        let falling = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL, 0.0),
            is_grounded: false,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(falling), None);

        let landed = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL, 0.0),
            is_grounded: true,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(landed), Some(PlayerFootstep::Landing));
        // The next grounded frame is an ordinary standing frame, not a second
        // landing.
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn stepping_off_a_lip_does_not_thud() {
        let mut footsteps = PlayerFootsteps::new();
        let hop = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL * 0.2, 0.0),
            is_grounded: false,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(hop), None);
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn a_landing_restarts_the_stride() {
        let mut footsteps = PlayerFootsteps::new();
        // Almost a full stride, then a fall.
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.9)), None);
        let falling = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL, 0.0),
            is_grounded: false,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(falling), None);
        let landed = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL, 0.0),
            is_grounded: true,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(landed), Some(PlayerFootstep::Landing));
        // The banked 0.9 stride is gone, so one more short frame is silent.
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.2)), None);
    }

    #[test]
    fn crouching_spaces_footsteps_further_apart() {
        let mut standing = PlayerFootsteps::new();
        let standing_steps = count_steps(&mut standing, 1005, STRIDE_DISTANCE / 100.0);

        let mut crouched = PlayerFootsteps::new();
        let crouched_steps = (0..1005)
            .filter(|_| {
                crouched.update(FootstepFrame {
                    self_translation: vec3(0.0, 0.0, STRIDE_DISTANCE / 100.0),
                    is_grounded: true,
                    is_crouched: true,
                }) == Some(PlayerFootstep::Step)
            })
            .count();

        assert!(
            crouched_steps < standing_steps,
            "crouched {crouched_steps} should be quieter than standing {standing_steps}"
        );
    }

    #[test]
    fn a_reset_forgets_the_stride_in_progress() {
        let mut footsteps = PlayerFootsteps::new();
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.9)), None);
        footsteps.reset();
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.2)), None);
    }

    #[test]
    fn a_reset_mid_fall_does_not_thud_on_arrival() {
        let mut footsteps = PlayerFootsteps::new();
        let falling = FootstepFrame {
            self_translation: vec3(0.0, -LANDING_MIN_FALL * 10.0, 0.0),
            is_grounded: false,
            is_crouched: false,
        };
        assert_eq!(footsteps.update(falling), None);
        footsteps.reset();
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn the_landing_query_asks_for_the_most_specific_match() {
        let effect = player_footstep_effect(PlayerFootstep::Landing, vec3(0.0, 0.0, 0.0));
        let Effect::PlayEnvironmentalSound { query, .. } = effect else {
            panic!("expected an environmental sound");
        };
        assert!(query.prefers_most_specific());
        assert!(
            query
                .tag_values()
                .contains(&("landing".to_owned(), "true".to_owned()))
        );
    }

    #[test]
    fn the_step_query_names_the_player_and_a_material() {
        let effect = player_footstep_effect(PlayerFootstep::Step, vec3(0.0, 0.0, 0.0));
        let Effect::PlayEnvironmentalSound { query, .. } = effect else {
            panic!("expected an environmental sound");
        };
        let tags = query.tag_values();
        assert!(tags.contains(&("event".to_owned(), "footstep".to_owned())));
        assert!(tags.contains(&("creaturetype".to_owned(), "player".to_owned())));
        // A player query with no material resolves to nothing at all.
        assert!(tags.iter().any(|(tag, _)| tag == "material"));
    }
}
