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
//! Two known gaps, both deliberate. Room-scale VR walking never moves the
//! capsule (headset translation only places the camera), so physically
//! stepping in the playspace is silent - the deck is not moving under the
//! player. And "own power" here means only "not carried by a platform": motion
//! the walk pass produces without the player asking for it - sliding down a
//! slope, being shoved by a door - still paces steps.

use cgmath::{InnerSpace, Vector3};
use dark::SCALE_FACTOR;
use engine::audio::AudioHandle;

use crate::{mission::PLAYER_MOVE_SPEED, scripts::Effect, scripts::script_util};

/// Footsteps per second the stride below is calibrated for, at unobstructed
/// full-speed movement. The port's player travels ~9.7 world units/second
/// (measured walking medsci1 in the open), which is a run rather than a walk,
/// so this matches the ~3.5/s the creature half measured for a jogging hybrid
/// rather than a strolling pace.
const TARGET_FOOTSTEPS_PER_SECOND: f32 = 3.5;

/// How far the player walks per footstep, in world units. Derived from the
/// movement speed so the cadence cannot drift if that is retuned; a player
/// moving at any other speed simply steps proportionally more or less often,
/// which is the whole point of pacing on distance.
const STRIDE_DISTANCE: f32 = PLAYER_MOVE_SPEED / SCALE_FACTOR / TARGET_FOOTSTEPS_PER_SECOND;

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
    /// A ladder climb or a scripted mantle. Such a player is holding a
    /// surface rather than walking on one - and, crucially, is not falling
    /// however far down the shaft they travel.
    pub is_climbing: bool,
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
    /// Swallow the next touchdown. Set by [`PlayerFootsteps::reset`], because
    /// a relocation can put the player in the air (a spawn point above the
    /// floor, a teleport onto a lip) and the settle that follows is the
    /// engine placing them, not a fall they took.
    suppress_next_landing: bool,
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
            suppress_next_landing: true,
        }
    }

    /// Forget the stride and the fall in progress. Called on any direct
    /// relocation (teleport locomotion, level load, quickload) so the player
    /// never arrives owing a footstep from wherever they were before.
    pub fn reset(&mut self) {
        self.distance = 0.0;
        self.fall_distance = 0.0;
        // Assume grounded so the arrival frame is not read as a landing, and
        // swallow the touchdown that follows an arrival made in mid-air.
        self.was_grounded = true;
        self.suppress_next_landing = true;
    }

    /// Advance one movement frame, returning the footstep it produced (at most
    /// one per frame).
    pub fn update(&mut self, frame: FootstepFrame) -> Option<PlayerFootstep> {
        let was_grounded = self.was_grounded;
        self.was_grounded = frame.is_grounded;

        if frame.is_climbing {
            // A ladder is held, not walked or fallen down. Without this a
            // descent accrues the whole shaft as fall distance and thuds at
            // the bottom, and a climb past a grounded pose near the foot of
            // the ladder paces strides out of vertical travel.
            self.fall_distance = 0.0;
            self.distance = 0.0;
            return None;
        }

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
            let suppressed = self.suppress_next_landing;
            self.fall_distance = 0.0;
            self.distance = 0.0;
            self.suppress_next_landing = false;
            return (!suppressed && fall_distance >= LANDING_MIN_FALL)
                .then_some(PlayerFootstep::Landing);
        }

        let stride = if frame.is_crouched {
            STRIDE_DISTANCE * CROUCH_STRIDE_MULTIPLIER
        } else {
            STRIDE_DISTANCE
        };
        // Walking proves the player is on the floor: whatever arrival the
        // reset was guarding against is over, so a later fall thuds normally.
        self.suppress_next_landing = false;
        let horizontal =
            cgmath::vec2(frame.self_translation.x, frame.self_translation.z).magnitude();
        self.distance += horizontal;
        if self.distance < stride {
            return None;
        }
        // Subtract rather than zero, so an ordinary frame does not lose its
        // remainder and drift the cadence. One step per frame is plenty - a
        // single frame covering several strides is a hitch, and a burst of
        // footsteps is the wrong way to report one - so a remainder bigger
        // than the stride is banked as a *part* stride rather than kept whole,
        // which would fire again on the very next frame that moves at all.
        self.distance = (self.distance - stride).min(stride * 0.5);
        Some(PlayerFootstep::Step)
    }
}

/// The schema query for one player footstep on `material`.
fn player_footstep_query(footstep: PlayerFootstep, material: &str) -> dark::EnvSoundQuery {
    let mut tags = vec![
        ("event", "footstep"),
        ("creaturetype", "player"),
        ("material", material),
    ];
    if footstep == PlayerFootstep::Landing {
        tags.push(("landing", "true"));
    }
    let query = dark::EnvSoundQuery::from_tag_values(tags);
    if footstep == PlayerFootstep::Landing {
        query.most_specific()
    } else {
        query
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
///
/// `ground_material` is the deck underfoot, from the world geometry's own
/// texture material - carpet reads `ftcar*`, tile `fttil*`, bulkhead
/// `ftmet*`. `None` (no ground found, or a surface with no material) keeps the
/// default. The schema does not author every material for every sub-key, so
/// the query falls back to the default material rather than going silent on,
/// say, a landing the schema has no thud for.
pub fn player_footstep_effect(
    footstep: PlayerFootstep,
    position: Vector3<f32>,
    ground_material: Option<&str>,
) -> Effect {
    let material = ground_material.unwrap_or(script_util::DEFAULT_IMPACT_MATERIAL);
    let query = player_footstep_query(footstep, material);
    let query = if material == script_util::DEFAULT_IMPACT_MATERIAL {
        query
    } else {
        query.with_fallback(player_footstep_query(
            footstep,
            script_util::DEFAULT_IMPACT_MATERIAL,
        ))
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
            is_climbing: false,
        }
    }

    fn crouch_walking(distance: f32) -> FootstepFrame {
        FootstepFrame {
            is_crouched: true,
            ..walking(distance)
        }
    }

    /// One airborne frame dropping `drop` world units.
    fn falling(drop: f32) -> FootstepFrame {
        FootstepFrame {
            self_translation: vec3(0.0, -drop, 0.0),
            is_grounded: false,
            is_crouched: false,
            is_climbing: false,
        }
    }

    /// The frame that touches down at the end of a `drop`-unit descent.
    fn landed(drop: f32) -> FootstepFrame {
        FootstepFrame {
            is_grounded: true,
            ..falling(drop)
        }
    }

    /// One frame of ladder climbing, moving `travel` world units vertically.
    fn climbing(travel: f32) -> FootstepFrame {
        FootstepFrame {
            self_translation: vec3(0.0, travel, 0.0),
            is_grounded: false,
            is_crouched: false,
            is_climbing: true,
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
    fn full_speed_walking_paces_the_target_cadence() {
        // The absolute check: one second of unobstructed full-speed movement,
        // fed frame by frame exactly as `MissionCore::update` builds it.
        let mut footsteps = PlayerFootsteps::new();
        let per_frame = PLAYER_MOVE_SPEED / SCALE_FACTOR / 60.0;
        let steps = count_steps(&mut footsteps, 60, per_frame);
        assert!(
            (3..=4).contains(&steps),
            "expected ~{TARGET_FOOTSTEPS_PER_SECOND} footsteps in a second of full-speed walking, got {steps}"
        );
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
        assert!((0..600).all(|_| footsteps.update(walking(0.0)).is_none()));
    }

    #[test]
    fn airborne_travel_plays_no_footstep() {
        let mut footsteps = PlayerFootsteps::new();
        let jumping = FootstepFrame {
            self_translation: vec3(0.0, 0.1, STRIDE_DISTANCE),
            is_grounded: false,
            is_crouched: false,
            is_climbing: false,
        };
        assert!((0..60).all(|_| footsteps.update(jumping).is_none()));
    }

    #[test]
    fn landing_after_a_fall_fires_once() {
        let mut footsteps = PlayerFootsteps::new();
        // Walk first: an arrival is only suppressed until the player is seen
        // standing on something.
        assert_eq!(footsteps.update(walking(0.0)), None);
        assert_eq!(footsteps.update(falling(LANDING_MIN_FALL)), None);
        assert_eq!(
            footsteps.update(landed(LANDING_MIN_FALL)),
            Some(PlayerFootstep::Landing)
        );
        // The next grounded frame is an ordinary standing frame, not a second
        // landing.
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn stepping_off_a_lip_does_not_thud() {
        let mut footsteps = PlayerFootsteps::new();
        assert_eq!(footsteps.update(walking(0.0)), None);
        assert_eq!(footsteps.update(falling(LANDING_MIN_FALL * 0.2)), None);
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn a_landing_restarts_the_stride() {
        let mut footsteps = PlayerFootsteps::new();
        // Almost a full stride, then a fall.
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.9)), None);
        assert_eq!(footsteps.update(falling(LANDING_MIN_FALL)), None);
        assert_eq!(
            footsteps.update(landed(LANDING_MIN_FALL)),
            Some(PlayerFootstep::Landing)
        );
        // The banked 0.9 stride is gone, so one more short frame is silent.
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.2)), None);
    }

    #[test]
    fn a_hitch_frame_does_not_fire_twice_in_a_row() {
        // A frame covering several strides reports one footstep, and does not
        // leave a full stride banked that fires again immediately.
        let mut footsteps = PlayerFootsteps::new();
        assert_eq!(
            footsteps.update(walking(STRIDE_DISTANCE * 5.0)),
            Some(PlayerFootstep::Step)
        );
        assert_eq!(footsteps.update(walking(STRIDE_DISTANCE * 0.1)), None);
    }

    #[test]
    fn climbing_a_ladder_neither_steps_nor_thuds() {
        let mut footsteps = PlayerFootsteps::new();
        assert_eq!(footsteps.update(walking(0.0)), None);
        // A long descent: without the climb gate this accrues the whole shaft
        // as fall distance and thuds when the player reaches the bottom.
        assert!((0..120).all(|_| footsteps.update(climbing(-LANDING_MIN_FALL)).is_none()));
        assert_eq!(footsteps.update(walking(0.0)), None);
    }

    #[test]
    fn crouching_spaces_footsteps_further_apart() {
        let mut standing = PlayerFootsteps::new();
        let standing_steps = count_steps(&mut standing, 1005, STRIDE_DISTANCE / 100.0);

        let mut crouched = PlayerFootsteps::new();
        let crouched_steps = (0..1005)
            .filter(|_| {
                crouched.update(crouch_walking(STRIDE_DISTANCE / 100.0))
                    == Some(PlayerFootstep::Step)
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
    fn a_relocation_that_lands_in_mid_air_does_not_thud_on_arrival() {
        // Level load / quickload / a teleport onto a lip: the settle that
        // follows the arrival is the engine placing the player, not a fall.
        let mut footsteps = PlayerFootsteps::new();
        footsteps.reset();
        assert!((0..30).all(|_| footsteps.update(falling(LANDING_MIN_FALL)).is_none()));
        assert_eq!(footsteps.update(landed(LANDING_MIN_FALL)), None);
        // A genuine fall afterwards still thuds.
        assert_eq!(footsteps.update(walking(0.0)), None);
        assert_eq!(footsteps.update(falling(LANDING_MIN_FALL)), None);
        assert_eq!(
            footsteps.update(landed(LANDING_MIN_FALL)),
            Some(PlayerFootstep::Landing)
        );
    }

    #[test]
    fn the_landing_query_asks_for_the_most_specific_match() {
        let effect = player_footstep_effect(PlayerFootstep::Landing, vec3(0.0, 0.0, 0.0), None);
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

    /// The deck underfoot picks the sample: carpet is `ftcar*`, not the
    /// bulkhead `ftmet*` every step used to resolve. The default stays the
    /// fallback, so a material the schema never authored for this sub-key
    /// still makes a sound.
    #[test]
    fn a_step_names_the_ground_it_landed_on() {
        let effect =
            player_footstep_effect(PlayerFootstep::Step, vec3(0.0, 0.0, 0.0), Some("fabric"));
        let Effect::PlayEnvironmentalSound { query, .. } = effect else {
            panic!("expected an environmental sound");
        };
        assert!(
            query
                .tag_values()
                .contains(&("material".to_owned(), "fabric".to_owned())),
            "the real deck is queried first: {:?}",
            query.tag_values()
        );
        let fallback = query.fallback().expect("a fallback query");
        assert!(
            fallback
                .tag_values()
                .contains(&("material".to_owned(), "metal".to_owned())),
            "the default material backs it up: {:?}",
            fallback.tag_values()
        );
    }

    /// No ground found (mid-air, or a surface with no material) is the old
    /// behavior exactly, with no fallback to itself.
    #[test]
    fn a_step_with_no_ground_material_keeps_the_default() {
        let effect = player_footstep_effect(PlayerFootstep::Step, vec3(0.0, 0.0, 0.0), None);
        let Effect::PlayEnvironmentalSound { query, .. } = effect else {
            panic!("expected an environmental sound");
        };
        assert!(
            query
                .tag_values()
                .contains(&("material".to_owned(), "metal".to_owned())),
        );
        assert!(query.fallback().is_none());
    }

    #[test]
    fn the_step_query_names_the_player_and_a_material() {
        let effect = player_footstep_effect(PlayerFootstep::Step, vec3(0.0, 0.0, 0.0), None);
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
