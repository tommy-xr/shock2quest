//! Hand-anchored climbing: a gripping VR hand pins itself to the hold it
//! grabbed and the body moves inversely underneath it.
//!
//! State only - the grip probe and the resulting body motion belong to
//! `physics`. Owned by `VrInteraction`, which already drives both hands.

use cgmath::{InnerSpace, Quaternion, Vector3};

use crate::{physics::ClimbGrip, virtual_hand::hand_world_position, vr_config::Handedness};

/// Squeeze value a hand must cross to grab, matching `VirtualHand`'s grab edge.
const GRIP_SQUEEZE_THRESHOLD: f32 = 0.5;

/// How far a gripping hand may drift from the point it grabbed before the grip
/// breaks. The drift IS the body's failure to follow: the body is cast every
/// frame at exactly the offset the hand opened up, so a persistent gap means
/// something blocked it. Roughly Dark's 2 ft climb-detach distance.
pub const CLIMB_STRETCH_BREAK: f32 = 0.6;

/// One hand's hold: what it grabbed, and where the hand was in the world when
/// it did. The body is moved to keep the hand back at that point.
#[derive(Clone, Copy, Debug)]
pub struct GripAnchor {
    pub grip: ClimbGrip,
    pub hand_world_at_grab: Vector3<f32>,
}

/// One hand's per-frame climb input.
pub struct ClimbHandInput {
    /// Controller position in pawn space, as `InputContext` reports it.
    pub local_position: Vector3<f32>,
    pub squeeze: f32,
    /// False while the hand holds an item - a full hand cannot take a hold.
    pub is_empty: bool,
}

/// Which hand each slot of [`HandClimb::grips`] belongs to.
const HANDS: [Handedness; 2] = [Handedness::Left, Handedness::Right];

fn slot(hand: Handedness) -> usize {
    match hand {
        Handedness::Left => 0,
        Handedness::Right => 1,
    }
}

/// The body translation that puts a gripping hand back on the hold it grabbed.
///
/// Hands are tracked in pawn space, so moving the pawn moves the hand with it:
/// the translation that cancels a hand's drift is exactly that drift, negated.
pub fn anchored_body_translation(
    hand_world_at_grab: Vector3<f32>,
    pawn_pos: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    hand_local_now: Vector3<f32>,
) -> Vector3<f32> {
    hand_world_at_grab - hand_world_position(pawn_pos, pawn_rotation, hand_local_now)
}

/// Which hands hold a climb hold, and which one currently moves the body.
#[derive(Default)]
pub struct HandClimb {
    grips: [Option<GripAnchor>; 2],
    anchor: Option<Handedness>,
    /// Last frame's squeeze state, so a grab needs a fresh press: a hand that
    /// lost its hold (over-stretched, or the object went away) must not
    /// silently re-grab while the same squeeze is still held.
    was_squeezing: [bool; 2],
}

impl HandClimb {
    /// Advance one frame and return the body translation the anchor hand
    /// demands, or `None` when no hand holds anything.
    ///
    /// `probe` answers "what could a hand at this world point grab?" (see
    /// [`crate::physics::PhysicsWorld::climbable_grip_at`]) and `is_alive`
    /// whether a gripped entity still exists.
    pub fn update(
        &mut self,
        pawn_pos: Vector3<f32>,
        pawn_rotation: Quaternion<f32>,
        hands: [ClimbHandInput; 2],
        probe: impl Fn(Vector3<f32>) -> Option<ClimbGrip>,
        is_alive: impl Fn(shipyard::EntityId) -> bool,
    ) -> Option<Vector3<f32>> {
        let hand_world = hands
            .each_ref()
            .map(|hand| hand_world_position(pawn_pos, pawn_rotation, hand.local_position));

        for (index, hand) in hands.iter().enumerate() {
            let squeezing = hand.squeeze > GRIP_SQUEEZE_THRESHOLD;
            let was_squeezing = std::mem::replace(&mut self.was_squeezing[index], squeezing);
            match self.grips[index] {
                Some(anchor) => {
                    let over_stretched = (hand_world[index] - anchor.hand_world_at_grab)
                        .magnitude()
                        > CLIMB_STRETCH_BREAK;
                    let gone = anchor.grip.entity_id.is_some_and(|id| !is_alive(id));
                    if !squeezing || over_stretched || gone {
                        self.grips[index] = None;
                    }
                }
                None if squeezing && !was_squeezing && hand.is_empty => {
                    if let Some(grip) = probe(hand_world[index]) {
                        self.grips[index] = Some(GripAnchor {
                            grip,
                            hand_world_at_grab: hand_world[index],
                        });
                        // Last hand to grab drives the body.
                        self.anchor = Some(HANDS[index]);
                    }
                }
                None => {}
            }
        }

        // The anchor let go. Hand over to the other hand if it is still on -
        // re-based to where that hand is NOW, so the body continues from here
        // instead of snapping back to a grab point it has since travelled from.
        if self
            .anchor
            .is_some_and(|hand| self.grips[slot(hand)].is_none())
        {
            self.anchor = None;
            for index in 0..HANDS.len() {
                if let Some(anchor) = self.grips[index].as_mut() {
                    anchor.hand_world_at_grab = hand_world[index];
                    self.anchor = Some(HANDS[index]);
                }
            }
        }

        let index = slot(self.anchor?);
        let anchor = self.grips[index]?;
        Some(anchor.hand_world_at_grab - hand_world[index])
    }

    /// The hand currently moving the body, if any.
    pub fn anchor(&self) -> Option<Handedness> {
        self.anchor
    }

    /// Every hand that holds a hold, with the hold it holds.
    pub fn grips(&self) -> impl Iterator<Item = (Handedness, &GripAnchor)> {
        HANDS
            .iter()
            .enumerate()
            .filter_map(|(index, hand)| Some((*hand, self.grips[index].as_ref()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{ClimbGrip, ClimbGripKind};
    use cgmath::{Rotation3, Zero, vec3};

    fn ladder_grip(point: Vector3<f32>) -> ClimbGrip {
        ClimbGrip {
            kind: ClimbGripKind::Ladder,
            entity_id: None,
            point,
            normal: vec3(1.0, 0.0, 0.0),
        }
    }

    fn hand(local: Vector3<f32>, squeeze: f32) -> ClimbHandInput {
        ClimbHandInput {
            local_position: local,
            squeeze,
            is_empty: true,
        }
    }

    fn no_hand() -> ClimbHandInput {
        hand(Vector3::zero(), 0.0)
    }

    #[track_caller]
    fn assert_translation(actual: Option<Vector3<f32>>, expected: Vector3<f32>) {
        let actual = actual.expect("expected a gripping hand to move the body");
        assert!(
            (actual - expected).magnitude() < 1.0e-5,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn pulling_a_gripped_hand_down_lifts_the_body_by_the_same_amount() {
        let grab = vec3(-6.8, 2.6, 0.0);
        let pawn = vec3(-5.5, 1.5, 0.0);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        // The hand that grabbed `grab` from `pawn`, then pulled down 0.5.
        let pulled = grab - pawn - vec3(0.0, 0.5, 0.0);

        let translation = anchored_body_translation(grab, pawn, identity, pulled);
        assert!((translation - vec3(0.0, 0.5, 0.0)).magnitude() < 1.0e-5);
    }

    #[test]
    fn the_inverse_motion_follows_the_pawns_yaw() {
        // A pawn yawed 90 degrees turns a hand pulled along its local -Z into
        // a body translation along world +X: the hand's LOCAL offset is what
        // the pawn rotates, so the body has to answer in world space.
        let pawn = vec3(2.0, 0.0, 3.0);
        let yaw = Quaternion::from_angle_y(cgmath::Deg(90.0));
        let grabbed_local = vec3(0.0, 1.0, -1.0);
        let grab = hand_world_position(pawn, yaw, grabbed_local);

        let translation =
            anchored_body_translation(grab, pawn, yaw, grabbed_local + vec3(0.0, 0.0, -0.5));
        assert!(
            (translation - vec3(0.5, 0.0, 0.0)).magnitude() < 1.0e-5,
            "got {translation:?}"
        );
    }

    #[test]
    fn a_grab_anchors_the_hand_and_a_release_ends_the_climb() {
        let mut climb = HandClimb::default();
        let pawn = vec3(0.0, 0.0, 0.0);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);

        // The grab frame itself moves nothing - the hand is already there.
        let first = climb.update(
            pawn,
            identity,
            [no_hand(), hand(reach, 1.0)],
            |point| Some(ladder_grip(point)),
            |_| true,
        );
        assert_translation(first, Vector3::zero());
        assert_eq!(climb.anchor(), Some(Handedness::Right));

        // Pulling down lifts the body; nothing new is probed.
        let pull = climb.update(
            pawn,
            identity,
            [no_hand(), hand(reach - vec3(0.0, 0.4, 0.0), 1.0)],
            |_| None,
            |_| true,
        );
        assert_translation(pull, vec3(0.0, 0.4, 0.0));

        // Opening the hand drops the hold.
        let released = climb.update(
            pawn,
            identity,
            [no_hand(), hand(reach, 0.0)],
            |_| None,
            |_| true,
        );
        assert_eq!(released, None);
        assert_eq!(climb.anchor(), None);
        assert_eq!(climb.grips().count(), 0);
    }

    #[test]
    fn a_full_hand_and_a_held_squeeze_never_take_a_hold() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);

        let mut full = hand(reach, 1.0);
        full.is_empty = false;
        assert_eq!(
            climb.update(
                pawn,
                identity,
                [no_hand(), full],
                |p| Some(ladder_grip(p)),
                |_| true
            ),
            None,
        );

        // The squeeze was already down last frame, so an empty hand arriving
        // now still needs a fresh press.
        assert_eq!(
            climb.update(
                pawn,
                identity,
                [no_hand(), hand(reach, 1.0)],
                |p| Some(ladder_grip(p)),
                |_| true
            ),
            None,
        );
    }

    #[test]
    fn a_body_that_cannot_follow_stretches_the_grip_until_it_breaks() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        climb.update(
            pawn,
            identity,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );

        // The pawn never moves (blocked), so the hand's own travel is the
        // whole stretch. Just under the break is still a hold...
        let held = climb.update(
            pawn,
            identity,
            [
                no_hand(),
                hand(reach - vec3(0.0, CLIMB_STRETCH_BREAK - 0.05, 0.0), 1.0),
            ],
            |_| None,
            |_| true,
        );
        assert!(held.is_some());

        // ... and past it the hand comes off.
        assert_eq!(
            climb.update(
                pawn,
                identity,
                [
                    no_hand(),
                    hand(reach - vec3(0.0, CLIMB_STRETCH_BREAK + 0.05, 0.0), 1.0)
                ],
                |_| None,
                |_| true
            ),
            None,
        );
    }

    #[test]
    fn releasing_the_anchor_hands_off_to_the_other_hand_without_a_snap() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let right = vec3(0.3, 1.0, -1.0);
        let left = vec3(-0.3, 1.6, -1.0);

        climb.update(
            pawn,
            identity,
            [no_hand(), hand(right, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        climb.update(
            pawn,
            identity,
            [hand(left, 1.0), hand(right, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        assert_eq!(climb.anchor(), Some(Handedness::Left), "last grab wins");

        // Both hands travel while the left is the anchor, so the right's
        // original grab point is now stale by that travel.
        let travel = vec3(0.0, -0.4, 0.0);
        climb.update(
            pawn,
            identity,
            [hand(left + travel, 1.0), hand(right + travel, 1.0)],
            |_| None,
            |_| true,
        );

        // Letting the left go must not yank the body back to the right hand's
        // stale grab point: the handoff frame asks for no motion at all.
        let handoff = climb.update(
            pawn,
            identity,
            [hand(left + travel, 0.0), hand(right + travel, 1.0)],
            |_| None,
            |_| true,
        );
        assert_eq!(climb.anchor(), Some(Handedness::Right));
        assert_translation(handoff, Vector3::zero());

        // ... and the right hand then drives from where it actually is.
        let after = climb.update(
            pawn,
            identity,
            [
                hand(left + travel, 0.0),
                hand(right + travel - vec3(0.0, 0.2, 0.0), 1.0),
            ],
            |_| None,
            |_| true,
        );
        assert_translation(after, vec3(0.0, 0.2, 0.0));
    }

    #[test]
    fn a_grip_on_an_entity_that_went_away_is_dropped() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        let entity = shipyard::EntityId::from_inner(42).unwrap();

        climb.update(
            pawn,
            identity,
            [no_hand(), hand(reach, 1.0)],
            |point| {
                ClimbGrip {
                    entity_id: Some(entity),
                    ..ladder_grip(point)
                }
                .into()
            },
            |_| true,
        );
        assert_eq!(climb.grips().count(), 1);

        assert_eq!(
            climb.update(
                pawn,
                identity,
                [no_hand(), hand(reach, 1.0)],
                |_| None,
                |_| false
            ),
            None,
        );
    }
}
