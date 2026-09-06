//! Hand-anchored climbing: a gripping VR hand pins itself to the hold it
//! grabbed and the body moves inversely underneath it.
//!
//! State only - the grip probe and the resulting body motion belong to
//! `physics`. Owned by `VrInteraction`, which already drives both hands.

use std::collections::VecDeque;

use cgmath::{InnerSpace, Quaternion, Rotation, Vector3, Zero};

use crate::{
    physics::{ClimbGrip, ClimbGripKind},
    virtual_hand::hand_world_position,
    vr_config::Handedness,
};

/// How far a gripping hand may drift from the point it grabbed before the grip
/// breaks. The drift IS the body's failure to follow: the body is cast every
/// frame at exactly the offset the hand opened up, so a persistent gap means
/// something blocked it. Roughly Dark's 2 ft climb-detach distance.
pub const CLIMB_STRETCH_BREAK: f32 = 0.6;

/// How many frames of hand travel a release averages over. Long enough to ride
/// out one noisy tracked frame, short enough to still read as the flick the
/// player just made.
const RELEASE_SAMPLE_FRAMES: usize = 4;

/// Ceiling on the speed a release throws the body with (world units/s).
pub(crate) const CLIMB_RELEASE_MAX_SPEED: f32 = 12.0;

/// Tighter ceiling on the UPWARD component, at the ordinary jump's own launch
/// speed: a haul-and-let-go can never rise higher than a jump, so it can never
/// drop the player further than a jump either (SS2 scores falls - issue #802).
pub(crate) const CLIMB_RELEASE_MAX_UP_SPEED: f32 = crate::physics::PLAYER_JUMP_LAUNCH_SPEED;

/// Below this a release is just letting go: not worth putting the body into a
/// ballistic arc (world units/s).
pub(crate) const CLIMB_RELEASE_MIN_SPEED: f32 = 0.5;

/// How far the eye must clear a ledge's lip before the body vaults onto it
/// (world units). Small: the moment the head is over the top is the moment a
/// climber commits, and waiting longer just makes the pull feel sticky.
pub const VAULT_EYE_MARGIN: f32 = 0.1;

/// Whether the player has pulled far enough up a ledge to be thrown onto it.
///
/// The head-over-the-lip convention (Boneworks, Blade & Sorcery): once the eye
/// clears the surface the anchor hand is lying ON, the climb is over and the
/// scripted top-out takes the body the rest of the way. A ladder rail offers no
/// vault - a hand reaching over a ladder's top onto the deck behind it takes a
/// `Ledge` grip on that deck, and vaults from there.
///
/// `anchor_on_top_surface` is the hand's contact normal being walkable: a hand
/// hooked on a lip's vertical face is not yet on top of anything.
///
/// The vault has to be EARNED, hence `eye_y_at_grab`: the eye must have risen
/// past the lip while the hand held it. Without that, every crate, console and
/// railing a standing player can already see over would throw them on top of
/// it the instant they squeezed - leaning on a chest-high box is not a mantle.
pub fn vault_ready(
    eye_y: f32,
    eye_y_at_grab: f32,
    lip_y: f32,
    anchor_kind: ClimbGripKind,
    anchor_on_top_surface: bool,
) -> bool {
    anchor_kind == ClimbGripKind::Ledge
        && anchor_on_top_surface
        && eye_y_at_grab <= lip_y
        && eye_y > lip_y + VAULT_EYE_MARGIN
}

/// The body velocity a release throws the player with, from the anchor hand's
/// recent per-frame travel relative to the pawn (newest last).
///
/// Negated: a hand hauled DOWN past the body throws the body up, the VR
/// climbing convention (Climbey, Boneworks, Stride). `None` when there is
/// nothing to throw with.
///
/// `step_dt` is the timestep the resulting arc will be INTEGRATED with, not
/// the wall clock: one sample of travel per game update becomes one physics
/// step of flight, so dividing by anything else makes the throw faster or
/// slower than the pull that produced it at any frame rate but 60 Hz.
pub(crate) fn release_velocity(travel: &[Vector3<f32>], step_dt: f32) -> Option<Vector3<f32>> {
    if travel.is_empty() || step_dt <= 0.0 {
        return None;
    }
    let mean = travel
        .iter()
        .fold(Vector3::zero(), |sum, delta| sum + delta)
        / travel.len() as f32;
    let mut velocity = -mean / step_dt;
    let speed = velocity.magnitude();
    if speed > CLIMB_RELEASE_MAX_SPEED {
        velocity *= CLIMB_RELEASE_MAX_SPEED / speed;
    }
    velocity.y = velocity.y.min(CLIMB_RELEASE_MAX_UP_SPEED);
    (velocity.magnitude() > CLIMB_RELEASE_MIN_SPEED).then_some(velocity)
}

/// What one frame of hand climbing asks of the body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClimbFrame {
    /// Body translation the anchor hand demands, or `None` when no hand holds.
    pub translation: Option<Vector3<f32>>,
    /// Velocity to launch the body with, set on the frame the LAST hold is
    /// voluntarily released (see [`release_velocity`]).
    pub launch: Option<Vector3<f32>>,
}

/// One hand's hold: what it grabbed, and where the hand was in the world when
/// it did. The body is moved to keep the hand back at that point.
#[derive(Clone, Copy, Debug)]
pub struct GripAnchor {
    pub grip: ClimbGrip,
    pub hand_world_at_grab: Vector3<f32>,
    /// Where the body was when this hold was taken, so a vault can tell a pull
    /// from a grab (see [`vault_ready`]).
    pub pawn_at_grab: Vector3<f32>,
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
    /// A fresh squeeze may reach contact a few frames late; never retry a broken hold.
    grab_grace: [f32; 2],
    /// The anchor hand's recent travel relative to the pawn, in world axes,
    /// newest last - the pull, which a release throws the body with. Cleared
    /// when the anchor changes: the history belongs to the hand that made it.
    recent_anchor_travel: VecDeque<Vector3<f32>>,
    /// That hand's pawn-space position last frame, to difference against.
    last_anchor_local: Option<Vector3<f32>>,
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
        step_dt: f32,
        hands: [ClimbHandInput; 2],
        probe: impl Fn(Vector3<f32>) -> Option<ClimbGrip>,
        is_alive: impl Fn(shipyard::EntityId) -> bool,
    ) -> ClimbFrame {
        let previous_anchor = self.anchor;
        let had_a_hold = self.grips.iter().any(Option::is_some);
        // Per hand: was this the player opening their hand? A hold torn off by
        // an over-stretched (blocked) body, or by the entity going away,
        // recorded travel that was the body failing to follow, not a pull -
        // and one hand's break must not swallow the other hand's throw.
        let mut let_go_cleanly = [true; 2];
        let hand_world = hands
            .each_ref()
            .map(|hand| hand_world_position(pawn_pos, pawn_rotation, hand.local_position));

        // Sample the anchor's travel BEFORE the releases below, so the flick
        // on the very frame the hand opens is part of what throws the body.
        if let Some(hand) = previous_anchor {
            let local = hands[slot(hand)].local_position;
            match self.last_anchor_local.replace(local) {
                Some(previous) => {
                    if self.recent_anchor_travel.len() == RELEASE_SAMPLE_FRAMES {
                        self.recent_anchor_travel.pop_front();
                    }
                    self.recent_anchor_travel
                        .push_back(pawn_rotation.rotate_vector(local - previous));
                }
                None => self.recent_anchor_travel.clear(),
            }
        }

        for (index, hand) in hands.iter().enumerate() {
            let squeezing = hand.squeeze > crate::ui::VR_TRIGGER_THRESHOLD;
            let was_squeezing = std::mem::replace(&mut self.was_squeezing[index], squeezing);
            if !squeezing || !hand.is_empty {
                self.grab_grace[index] = 0.0;
            } else if !was_squeezing {
                self.grab_grace[index] = 0.15;
            }
            match self.grips[index] {
                Some(anchor) => {
                    let over_stretched = (hand_world[index] - anchor.hand_world_at_grab)
                        .magnitude()
                        > CLIMB_STRETCH_BREAK;
                    let gone = anchor.grip.entity_id.is_some_and(|id| !is_alive(id));
                    // The hands update after this, so the same squeeze that
                    // took a hold can also close on an item; a full hand lets
                    // go of the ladder rather than holding both.
                    if !squeezing || !hand.is_empty || over_stretched || gone {
                        self.grab_grace[index] = 0.0;
                        self.grips[index] = None;
                        let_go_cleanly[index] = !squeezing && !over_stretched && !gone;
                    }
                }
                None if self.grab_grace[index] > 0.0 => {
                    if let Some(grip) = probe(hand_world[index]) {
                        self.grab_grace[index] = 0.0;
                        self.grips[index] = Some(GripAnchor {
                            grip,
                            hand_world_at_grab: hand_world[index],
                            pawn_at_grab: pawn_pos,
                        });
                        // Last hand to grab drives the body.
                        self.anchor = Some(HANDS[index]);
                    }
                }
                None => {}
            }
            self.grab_grace[index] = (self.grab_grace[index] - step_dt.max(0.0)).max(0.0);
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

        let launch = if had_a_hold
            && self.anchor.is_none()
            && previous_anchor.is_some_and(|hand| let_go_cleanly[slot(hand)])
        {
            release_velocity(self.recent_anchor_travel.make_contiguous(), step_dt)
        } else {
            None
        };

        // A new anchor starts a fresh history: the travel so far is the other
        // hand's, and a re-based hold is a new pull.
        if self.anchor != previous_anchor {
            self.recent_anchor_travel.clear();
            self.last_anchor_local = self.anchor.map(|hand| hands[slot(hand)].local_position);
        }

        let translation = self.anchor.and_then(|hand| {
            let index = slot(hand);
            let anchor = self.grips[index]?;
            Some(anchored_body_translation(
                anchor.hand_world_at_grab,
                pawn_pos,
                pawn_rotation,
                hands[index].local_position,
            ))
        });
        ClimbFrame {
            translation,
            launch,
        }
    }

    /// The hand currently moving the body, if any.
    pub fn anchor(&self) -> Option<Handedness> {
        self.anchor
    }

    /// The hold the anchor hand is on, if any.
    pub fn anchor_grip(&self) -> Option<&GripAnchor> {
        self.grips[slot(self.anchor?)].as_ref()
    }

    /// Whether any hand is on a ledge - the body balls up while it is.
    pub fn holds_a_ledge(&self) -> bool {
        self.grips()
            .any(|(_, anchor)| anchor.grip.kind == ClimbGripKind::Ledge)
    }

    /// Drop every hold without throwing the body: the vault took over, and it
    /// is not the player letting go. The still-closed squeeze cannot re-grab
    /// (a grab needs a fresh press), so the hands stay out of the way until
    /// the scripted top-out has finished.
    pub fn release_all(&mut self) {
        self.grips = [None, None];
        self.grab_grace = [0.0, 0.0];
        self.anchor = None;
        self.recent_anchor_travel.clear();
        self.last_anchor_local = None;
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

    /// One 60 Hz frame.
    const DT: f32 = 1.0 / 60.0;

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
    fn assert_translation(actual: ClimbFrame, expected: Vector3<f32>) {
        let actual = actual
            .translation
            .expect("expected a gripping hand to move the body");
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
            DT,
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
            DT,
            [no_hand(), hand(reach - vec3(0.0, 0.4, 0.0), 1.0)],
            |_| None,
            |_| true,
        );
        assert_translation(pull, vec3(0.0, 0.4, 0.0));

        // Opening the hand drops the hold.
        let released = climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 0.0)],
            |_| None,
            |_| true,
        );
        assert_eq!(released.translation, None);
        assert_eq!(climb.anchor(), None);
        assert_eq!(climb.grips().count(), 0);
    }

    #[test]
    fn a_fresh_squeeze_can_reach_contact_late_but_cannot_regrab_after_a_break() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        for _ in 0..3 {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(reach, 1.0)],
                |_| None,
                |_| true,
            );
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(climb.anchor().is_some());
        let broken = reach + vec3(0.0, 1.0, 0.0);
        for _ in 0..3 {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(broken, 1.0)],
                |p| Some(ladder_grip(p)),
                |_| true,
            );
            assert!(climb.anchor().is_none());
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 0.0)],
            |_| None,
            |_| true,
        );
        for _ in 0..12 {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(reach, 1.0)],
                |_| None,
                |_| true,
            );
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(
            climb.anchor().is_none(),
            "expired grace is not a permanent auto-grab"
        );
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
            climb
                .update(
                    pawn,
                    identity,
                    DT,
                    [no_hand(), full],
                    |p| Some(ladder_grip(p)),
                    |_| true
                )
                .translation,
            None,
        );

        // The squeeze was already down last frame, so an empty hand arriving
        // now still needs a fresh press.
        assert_eq!(
            climb
                .update(
                    pawn,
                    identity,
                    DT,
                    [no_hand(), hand(reach, 1.0)],
                    |p| Some(ladder_grip(p)),
                    |_| true
                )
                .translation,
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
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );

        // The pawn never moves (blocked), so the hand's own travel is the
        // whole stretch. Just under the break is still a hold...
        let held = climb.update(
            pawn,
            identity,
            DT,
            [
                no_hand(),
                hand(reach - vec3(0.0, CLIMB_STRETCH_BREAK - 0.05, 0.0), 1.0),
            ],
            |_| None,
            |_| true,
        );
        assert!(held.translation.is_some());

        // ... and past it the hand comes off.
        assert_eq!(
            climb
                .update(
                    pawn,
                    identity,
                    DT,
                    [
                        no_hand(),
                        hand(reach - vec3(0.0, CLIMB_STRETCH_BREAK + 0.05, 0.0), 1.0)
                    ],
                    |_| None,
                    |_| true
                )
                .translation,
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
            DT,
            [no_hand(), hand(right, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        climb.update(
            pawn,
            identity,
            DT,
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
            DT,
            [hand(left + travel, 1.0), hand(right + travel, 1.0)],
            |_| None,
            |_| true,
        );

        // Letting the left go must not yank the body back to the right hand's
        // stale grab point: the handoff frame asks for no motion at all.
        let handoff = climb.update(
            pawn,
            identity,
            DT,
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
            DT,
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
    fn a_release_throws_the_body_against_the_pull() {
        // A hand hauled down 0.1 wu per frame throws the body up at 6 wu/s.
        let velocity = release_velocity(&[vec3(0.0, -0.1, 0.0); 4], DT)
            .expect("a real pull should throw the body");
        assert!(
            (velocity - vec3(0.0, 6.0, 0.0)).magnitude() < 1.0e-4,
            "{velocity:?}"
        );

        // The mean is what counts: one frame of tracking noise cannot double it.
        let averaged = release_velocity(
            &[
                vec3(0.0, -0.1, 0.0),
                vec3(0.0, -0.3, 0.0),
                vec3(0.0, -0.1, 0.0),
                vec3(0.0, 0.1, 0.0),
            ],
            DT,
        )
        .expect("a real pull should throw the body");
        assert!((averaged.y - 6.0).abs() < 1.0e-4, "{averaged:?}");
    }

    #[test]
    fn a_release_is_clamped_to_jump_speed_and_never_lofts_higher() {
        // A yank far faster than anything a jump does.
        let sideways = release_velocity(&[vec3(-1.0, 0.0, 0.0)], DT).unwrap();
        assert!(
            (sideways.magnitude() - CLIMB_RELEASE_MAX_SPEED).abs() < 1.0e-4,
            "{sideways:?}"
        );
        assert!(sideways.x > 0.0, "the throw opposes the pull: {sideways:?}");

        // The up cap is tighter than the magnitude clamp, so a near-vertical
        // yank loses the excess rise and keeps its horizontal reach.
        let steep = release_velocity(&[vec3(0.0, -1.0, -0.15)], DT).unwrap();
        assert!(
            (steep.y - CLIMB_RELEASE_MAX_UP_SPEED).abs() < 1.0e-4,
            "{steep:?}"
        );
        assert!(steep.z > 1.0, "{steep:?}");
    }

    #[test]
    fn barely_moving_hands_and_a_frozen_frame_throw_nothing() {
        assert_eq!(release_velocity(&[vec3(0.0, -0.001, 0.0); 4], DT), None);
        assert_eq!(release_velocity(&[], DT), None);
        assert_eq!(release_velocity(&[vec3(0.0, -0.1, 0.0)], 0.0), None);
    }

    /// Grab, pull `travel` per frame for `frames`, then end the grip the way
    /// `finish` says. Returns the frame the last hold came off on.
    fn pull_and_finish(
        frames: usize,
        travel: Vector3<f32>,
        mut finish: impl FnMut(&mut HandClimb, Vector3<f32>) -> ClimbFrame,
    ) -> ClimbFrame {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        let mut at = reach;
        for _ in 0..frames {
            at += travel;
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 1.0)],
                |_| None,
                |_| true,
            );
        }
        finish(&mut climb, at)
    }

    fn open_the_hand(climb: &mut HandClimb, at: Vector3<f32>) -> ClimbFrame {
        climb.update(
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            DT,
            [no_hand(), hand(at, 0.0)],
            |_| None,
            |_| true,
        )
    }

    #[test]
    fn letting_go_of_the_last_hold_launches_the_body() {
        // A quick haul: 0.1 wu of hand travel per frame, let go mid-pull.
        let released = pull_and_finish(4, vec3(0.0, -0.1, 0.0), |climb, at| {
            open_the_hand(climb, at + vec3(0.0, -0.1, 0.0))
        });
        let launch = released.launch.expect("a quick pull should launch");
        assert!((launch.y - 6.0).abs() < 0.1, "{launch:?}");
        assert_eq!(released.translation, None);

        // Stopping the hand before opening it is part of the pull: the same
        // haul, released from rest, throws proportionally less.
        let stopped = pull_and_finish(4, vec3(0.0, -0.1, 0.0), open_the_hand);
        let stopped = stopped.launch.expect("a quick pull should launch");
        assert!(stopped.y > 0.0 && stopped.y < launch.y, "{stopped:?}");
    }

    #[test]
    fn a_grip_torn_off_by_a_blocked_body_launches_nothing() {
        // Same hand travel, but the body never followed: the hand ran past the
        // stretch break instead of being opened, so the "pull" was the body
        // failing to move and there is nothing to throw it with.
        let mut broke = ClimbFrame::default();
        pull_and_finish(
            (CLIMB_STRETCH_BREAK / 0.1) as usize + 2,
            vec3(0.0, -0.1, 0.0),
            |climb, at| {
                broke = climb.update(
                    Vector3::zero(),
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    DT,
                    [no_hand(), hand(at, 1.0)],
                    |_| None,
                    |_| true,
                );
                broke
            },
        );
        assert_eq!(broke.translation, None, "the grip should have broken");
        assert_eq!(broke.launch, None);
    }

    #[test]
    fn handing_over_to_the_other_hand_is_not_a_release() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let right = vec3(0.3, 1.0, -1.0);
        let left = vec3(-0.3, 1.6, -1.0);
        let step = |climb: &mut HandClimb, hands: [ClimbHandInput; 2], probe: bool| {
            climb.update(
                pawn,
                identity,
                DT,
                hands,
                |p| probe.then(|| ladder_grip(p)),
                |_| true,
            )
        };
        step(&mut climb, [hand(left, 1.0), hand(right, 1.0)], true);
        let mut at = left;
        for _ in 0..4 {
            at += vec3(0.0, -0.1, 0.0);
            step(&mut climb, [hand(at, 1.0), hand(right, 1.0)], false);
        }
        // The left opens while the right is still on: the body keeps climbing,
        // it does not get thrown.
        let handoff = step(&mut climb, [hand(at, 0.0), hand(right, 1.0)], false);
        assert_eq!(handoff.launch, None);
        assert_eq!(climb.anchor(), Some(Handedness::Right));
    }

    #[test]
    fn the_eye_must_clear_the_lip_of_a_ledge_the_hand_is_lying_on() {
        // Grabbed from below the lip, pulled until the eye cleared it: vault.
        assert!(vault_ready(6.2, 5.0, 6.0, ClimbGripKind::Ledge, true));
        // Still below it, or only just level with it: keep pulling.
        assert!(!vault_ready(5.9, 5.0, 6.0, ClimbGripKind::Ledge, true));
        assert!(!vault_ready(
            6.0 + VAULT_EYE_MARGIN,
            5.0,
            6.0,
            ClimbGripKind::Ledge,
            true
        ));
        // Never pulled at all: a standing player who grabs a chest-high crate
        // was already looking over it, and leaning on it is not a mantle.
        assert!(!vault_ready(6.2, 6.2, 6.0, ClimbGripKind::Ledge, true));
        // Hooked on the lip's vertical face, not lying on the top.
        assert!(!vault_ready(6.2, 5.0, 6.0, ClimbGripKind::Ledge, false));
        // A ladder rail is never a vault, however high the eye gets.
        assert!(!vault_ready(9.0, 5.0, 6.0, ClimbGripKind::Ladder, true));
    }

    #[test]
    fn only_a_ledge_hold_balls_the_body_up_and_it_remembers_where_it_was_taken() {
        let mut climb = HandClimb::default();
        let pawn = vec3(0.0, 1.24, 0.0);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        let ledge = |point: Vector3<f32>| ClimbGrip {
            kind: ClimbGripKind::Ledge,
            normal: vec3(0.0, 1.0, 0.0),
            ..ladder_grip(point)
        };

        // A ladder rung is not something to ball up on.
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(!climb.holds_a_ledge());

        // The other hand takes a ledge: now it is, and the anchor records the
        // body pose that grab was made from - what tells a pull from a grab.
        climb.update(
            pawn,
            identity,
            DT,
            [hand(reach, 1.0), hand(reach, 1.0)],
            |p| Some(ledge(p)),
            |_| true,
        );
        assert!(climb.holds_a_ledge());
        assert_eq!(climb.anchor(), Some(Handedness::Left));
        assert_eq!(climb.anchor_grip().unwrap().pawn_at_grab, pawn);

        // Hauling the body up does not rewrite it: the pull is measured from
        // where the hold was taken, however far the body has since travelled.
        let lifted = pawn + vec3(0.0, 0.4, 0.0);
        climb.update(
            lifted,
            identity,
            DT,
            [hand(reach, 1.0), hand(reach, 1.0)],
            |_| None,
            |_| true,
        );
        assert_eq!(climb.anchor_grip().unwrap().pawn_at_grab, pawn);
    }

    #[test]
    fn a_vault_drops_every_hold_without_throwing_the_body() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let reach = vec3(0.0, 1.0, -1.0);
        let step = |climb: &mut HandClimb, at: Vector3<f32>, probe: bool| {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 1.0)],
                |p| probe.then(|| ladder_grip(p)),
                |_| true,
            )
        };
        step(&mut climb, reach, true);
        let mut at = reach;
        for _ in 0..4 {
            at += vec3(0.0, -0.1, 0.0);
            step(&mut climb, at, false);
        }

        climb.release_all();
        assert_eq!(climb.grips().count(), 0);
        assert_eq!(climb.anchor(), None);

        // The squeeze is still down, so the hand cannot take a new hold while
        // the scripted top-out is running, and nothing is thrown.
        let after = step(&mut climb, at, true);
        assert_eq!(after.translation, None);
        assert_eq!(after.launch, None);
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
            DT,
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
            climb
                .update(
                    pawn,
                    identity,
                    DT,
                    [no_hand(), hand(reach, 1.0)],
                    |_| None,
                    |_| false
                )
                .translation,
            None,
        );
    }
}
