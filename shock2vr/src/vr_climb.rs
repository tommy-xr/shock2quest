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

/// How far a ladder hand or passive hold may drift before the grip breaks.
/// An active ledge or surface hold instead consumes collision-rejected travel and stays held. The drift IS the body's failure to follow: the body is cast every
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

/// How far from the other hand's hold a contextual grab may land (world units).
const TRANSFER_REACH: f32 = 1.5;

/// Grace grabs one ladder departure allows: a surface beside the ladder, then
/// one more from that surface.
pub const GRACE_GRABS: u8 = 2;

/// Which rule [`HandClimb`] asks the grip probe to apply at a hand point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripProbe {
    /// A ladder or a ledge more than a step above the feet.
    Ordinary,
    /// Beside another hold: a deck may also sit just above the feet.
    Transfer,
    /// A grace grab: any static solid face (see [`GRACE_GRABS`]).
    Surface,
}

/// A deliberate pull commits a ledge hold once the tracked eye clears it.
/// A grab by itself never vaults, but grabbing after looking over the deck is
/// valid: what matters is pulling on this hold, not the eye height at acquisition.
pub fn vault_ready(
    eye_y: f32,
    pull_progress: f32,
    lip_y: f32,
    anchor_kind: ClimbGripKind,
    anchor_on_top_surface: bool,
) -> bool {
    anchor_kind == ClimbGripKind::Ledge
        && anchor_on_top_surface
        && pull_progress >= 0.15
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
pub(crate) fn release_velocity<'a>(
    travel: impl IntoIterator<Item = &'a Vector3<f32>>,
    step_dt: f32,
) -> Option<Vector3<f32>> {
    let (sum, count) = travel
        .into_iter()
        .fold((Vector3::zero(), 0), |(sum, count), delta| {
            (sum + delta, count + 1)
        });
    if count == 0 || step_dt <= 0.0 {
        return None;
    }
    let mean = sum / count as f32;
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
    /// The LAST hold was voluntarily opened this frame, thrown or not. A
    /// broken hold or a handoff to the other hand is not a release.
    pub released: bool,
}

/// One hand's hold: what it grabbed, and where the hand was in the world when
/// it did. The body is moved to keep the hand back at that point.
#[derive(Clone, Copy, Debug)]
pub struct GripAnchor {
    pub grip: ClimbGrip,
    /// Tracking reference; an active ledge adjusts this by rejected movement
    /// while `grip.point` continues to identify the actual held surface.
    pub hand_world_at_grab: Vector3<f32>,
    /// Where the body was when this hold was taken, so a vault can tell a pull
    /// from a grab (see [`vault_ready`]).
    pub pawn_at_grab: Vector3<f32>,
    /// Which grace grab took this hold (1 or 2); 0 for an ordinary hold.
    pub grace: u8,
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
pub(crate) const HANDS: [Handedness; 2] = [Handedness::Left, Handedness::Right];

/// A ladder, or a grace hold taken off one: holding either is a ladder exit.
fn is_ladder_exit_hold(kind: ClimbGripKind) -> bool {
    matches!(kind, ClimbGripKind::Ladder | ClimbGripKind::Surface)
}

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
    /// A fresh empty-hand squeeze stays armed until it takes a hold, so a hand
    /// closed before reaching the rung grabs on contact. A broken hold disarms
    /// it: the still-closed hand must not re-grab in a loop.
    armed: [bool; 2],
    /// The anchor hand's recent travel relative to the pawn, in world axes,
    /// newest last - the pull, which a release throws the body with. Cleared
    /// when the anchor changes: the history belongs to the hand that made it.
    recent_anchor_travel: VecDeque<Vector3<f32>>,
    /// That hand's pawn-space position last frame, to difference against.
    last_anchor_local: Option<Vector3<f32>>,
    /// Grace grabs spent since the last fresh ladder grab. Releasing a grace
    /// hold refunds nothing, so a held ladder cannot mint a second first grab
    /// - except one free re-grab of it (see `regrab_spent`).
    grace_used: u8,
    /// The one free re-grab of grace hold 1, taken while the other hand is
    /// still on the ladder, is spent. Resets with the allowance.
    regrab_spent: bool,
    /// Holds taken last frame. One the same squeeze lets go of for an item
    /// was a reach for the item, not a grace grab: it refunds.
    fresh: [bool; 2],
    was_grounded: bool,
}

impl HandClimb {
    /// Advance one frame and return the body translation the anchor hand
    /// demands, or `None` when no hand holds anything.
    ///
    /// `probe` answers "what could a hand at this world point grab?" under a
    /// [`GripProbe`] rule (see [`Self::probe_grip`]) and `is_alive` whether a
    /// gripped entity still exists.
    pub fn update(
        &mut self,
        pawn_pos: Vector3<f32>,
        pawn_rotation: Quaternion<f32>,
        step_dt: f32,
        hands: [ClimbHandInput; 2],
        probe: impl Fn(Vector3<f32>, GripProbe) -> Option<ClimbGrip>,
        is_alive: impl Fn(shipyard::EntityId) -> bool,
    ) -> ClimbFrame {
        let previous_anchor = self.anchor;
        let had_a_hold = self.grips.iter().any(Option::is_some);
        let held_before = self.grips;
        // Per hand: was this the player opening their hand? A hold torn off by
        // an over-stretched (blocked) body, or by the entity going away,
        // recorded travel that was the body failing to follow, not a pull -
        // and one hand's break must not swallow the other hand's throw.
        let mut let_go_cleanly = [true; 2];
        let hand_world = hands
            .each_ref()
            .map(|hand| hand_world_position(pawn_pos, pawn_rotation, hand.local_position));

        // Ladder releases include the final tracking flick. Ledge and surface throws use
        // collision-resolved held motion only: the opening frame has no held
        // movement to resolve, so its raw tracking delta cannot prove momentum.
        if let Some(hand) = previous_anchor.filter(|hand| {
            self.grips[slot(*hand)].is_none_or(|anchor| {
                anchor.grip.kind == ClimbGripKind::Ladder
                    || hands[slot(*hand)].squeeze > crate::ui::VR_TRIGGER_THRESHOLD
            })
        }) {
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
                self.armed[index] = false;
            } else if !was_squeezing {
                self.armed[index] = true;
            }
            match self.grips[index] {
                Some(anchor) => {
                    // A deck can block the hips while its hand stays closed.
                    // Keep that support until the player opens the hand; a
                    // blocked pull must not silently abandon a ladder handoff.
                    // Passive holds still break when the other hand carries
                    // the body away: they no longer provide reachable support.
                    let over_stretched = (anchor.grip.kind == ClimbGripKind::Ladder
                        || previous_anchor != Some(HANDS[index]))
                        && (hand_world[index] - anchor.hand_world_at_grab).magnitude()
                            > CLIMB_STRETCH_BREAK;
                    let gone = anchor.grip.entity_id.is_some_and(|id| !is_alive(id));
                    // The hands update after this, so the same squeeze that
                    // took a hold can also close on an item; a full hand lets
                    // go of the ladder rather than holding both.
                    if !squeezing || !hand.is_empty || over_stretched || gone {
                        if !hand.is_empty && self.fresh[index] && anchor.grace > 0 {
                            // A free re-grab refunds the re-grab, not grace 1.
                            if anchor.grace == 1 && self.regrab_spent {
                                self.regrab_spent = false;
                            } else {
                                self.grace_used = self.grace_used.min(anchor.grace - 1);
                            }
                        }
                        self.armed[index] = false;
                        self.grips[index] = None;
                        let_go_cleanly[index] = !squeezing && !over_stretched && !gone;
                    }
                }
                None => {}
            }
        }
        // Resolve both hands against the holds that survived the releases
        // above, before either acquires: a hold taken this frame cannot
        // authorize a contextual grab, whatever the hand order, and one let
        // go of this frame only carries the departure (see probe_grip_after);
        // one torn away (broken, gone, or the hand filled) carries nothing.
        self.fresh = [false; 2];
        let acquired: [_; 2] = std::array::from_fn(|index| {
            let released = held_before[1 - index]
                .filter(|_| self.grips[1 - index].is_none() && let_go_cleanly[1 - index]);
            (self.grips[index].is_none() && self.armed[index])
                .then(|| self.probe_grip_after(HANDS[index], hand_world[index], &probe, released))
                .flatten()
        });
        for (index, acquired) in acquired.into_iter().enumerate() {
            let Some((grip, grace)) = acquired else {
                continue;
            };
            if grip.kind == ClimbGripKind::Ladder {
                self.reset_grace();
            } else if grace == 1 && self.grace_used == 1 {
                self.regrab_spent = true;
            } else if grace > 0 {
                self.grace_used = grace;
            }
            self.armed[index] = false;
            self.fresh[index] = true;
            self.grips[index] = Some(GripAnchor {
                grip,
                hand_world_at_grab: hand_world[index],
                pawn_at_grab: pawn_pos,
                grace,
            });
            self.anchor = Some(HANDS[index]);
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
                    anchor.pawn_at_grab = pawn_pos;
                    self.anchor = Some(HANDS[index]);
                }
            }
        }

        let released = had_a_hold
            && self.anchor.is_none()
            && previous_anchor.is_some_and(|hand| let_go_cleanly[slot(hand)]);
        let launch = released
            .then(|| release_velocity(&self.recent_anchor_travel, step_dt))
            .flatten();

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
        if self.grips.iter().all(Option::is_none) {
            self.reset_grace();
        }
        ClimbFrame {
            translation,
            launch,
            released,
        }
    }

    /// While something else owns the body (a climb exit's fade), take no hold
    /// but keep tracking the squeezes, so one held through it is no fresh
    /// press afterwards.
    pub fn track_squeezes_only(&mut self, squeezes: [f32; 2]) {
        for (index, squeeze) in squeezes.into_iter().enumerate() {
            self.was_squeezing[index] = squeeze > crate::ui::VR_TRIGGER_THRESHOLD;
            self.armed[index] = false;
        }
    }

    /// The throw opening the anchor hand now would give, from its pull so far.
    pub fn predicted_release_velocity(&self, step_dt: f32) -> Option<Vector3<f32>> {
        release_velocity(&self.recent_anchor_travel, step_dt)
    }

    /// What a free `hand` at `point` would take, and which grace grab (0 for
    /// none) it would spend. The one rule for both acquisition and the glove's
    /// in-reach glow, so the glow never promises a grab that would fail.
    ///
    /// Near the other hand's hold, a deck just above the feet is an ordinary
    /// transfer. Off a ladder or grace hold (a ladder departure), though, only
    /// another rung is ordinary: anything else is a grace grab, so it never
    /// vaults - a floor the hand reaches anywhere, any other solid face only
    /// near the other hold. A ladder hold allows grace grab 1, and grace hold
    /// 1 allows grace grab 2; never a third.
    pub fn probe_grip(
        &self,
        hand: Handedness,
        point: Vector3<f32>,
        probe: impl Fn(Vector3<f32>, GripProbe) -> Option<ClimbGrip>,
    ) -> Option<(ClimbGrip, u8)> {
        self.probe_grip_after(hand, point, probe, None)
    }

    /// [`Self::probe_grip`], where the other hand let go of `released` this
    /// frame: that hold still makes this a ladder departure, and authorizes
    /// a grace grab on a floor the hand reaches anyway, but no face that only
    /// a surviving hold nearby could offer.
    fn probe_grip_after(
        &self,
        hand: Handedness,
        point: Vector3<f32>,
        probe: impl Fn(Vector3<f32>, GripProbe) -> Option<ClimbGrip>,
        released: Option<GripAnchor>,
    ) -> Option<(ClimbGrip, u8)> {
        // A hand still closed after a broken hold takes nothing until reopened.
        if self.was_squeezing[slot(hand)] && !self.armed[slot(hand)] {
            return None;
        }
        let held = self.grips[1 - slot(hand)];
        let other = held.or(released);
        let near = held.filter(|other| (point - other.grip.point).magnitude() <= TRANSFER_REACH);
        let reachable = probe(
            point,
            if near.is_some() {
                GripProbe::Transfer
            } else {
                GripProbe::Ordinary
            },
        );
        let departing = other.is_some_and(|other| is_ladder_exit_hold(other.grip.kind));
        if let Some(grip) =
            reachable.filter(|grip| !departing || grip.kind == ClimbGripKind::Ladder)
        {
            return Some((grip, 0));
        }
        let grace = match other.map(|other| (other.grip.kind, other.grace)) {
            Some((ClimbGripKind::Ladder, _)) => 1,
            Some((_, 1)) => 2,
            _ => return None,
        };
        // Grace 1 again, while a hand is still on the ladder: the one free
        // re-grab. A ladder let go of this frame does not count.
        let free_regrab = grace == 1
            && !self.regrab_spent
            && held.is_some_and(|held| held.grip.kind == ClimbGripKind::Ladder);
        if self.grace_used >= grace && !free_regrab {
            return None;
        }
        // A floor the hand can hold anyway, wherever it is; any other solid
        // face only near the other hold.
        reachable
            .map(|grip| ClimbGrip {
                kind: ClimbGripKind::Surface,
                ..grip
            })
            .or_else(|| near.and_then(|_| probe(point, GripProbe::Surface)))
            .map(|grip| (grip, grace))
    }

    /// Whether the body hangs off a ladder or its grace holds, with no
    /// ordinary ledge held: a ladder exit, which the release assist serves.
    pub fn on_ladder_departure(&self) -> bool {
        self.grips()
            .any(|(_, anchor)| is_ladder_exit_hold(anchor.grip.kind))
            && !self.holds_a_ledge()
    }

    /// Grace grabs still available to this departure (see [`GRACE_GRABS`]).
    /// Honest about a dead end: with grace hold 1 let go of and its free
    /// re-grab spent, nothing can authorize grace grab 2.
    pub fn grace_remaining(&self) -> u8 {
        let grace1_held = self.grips().any(|(_, anchor)| anchor.grace == 1);
        if self.grace_used == 1 && self.regrab_spent && !grace1_held {
            0
        } else {
            GRACE_GRABS - self.grace_used
        }
    }

    fn reset_grace(&mut self) {
        self.grace_used = 0;
        self.regrab_spent = false;
    }

    /// Landing ends the departure: the next climb starts with a full
    /// allowance. Only the landing - standing at a ladder's foot with a hand
    /// on it must not re-mint a first grace grab every frame.
    pub fn set_grounded(&mut self, grounded: bool) {
        if grounded && !self.was_grounded {
            self.reset_grace();
        }
        self.was_grounded = grounded;
    }

    /// Consume only the part of a ledge or surface pull that collision prevented. The
    /// actual surface and acquisition pose stay fixed, but rejected tracking
    /// travel must not accumulate into a sudden move when the obstruction
    /// clears. Ladder holds retain their existing stretch-and-break behavior.
    pub fn resolve_translation(&mut self, requested: Vector3<f32>, applied: Vector3<f32>) {
        let Some(anchor) = self.anchor.and_then(|hand| self.grips[slot(hand)].as_mut()) else {
            return;
        };
        if anchor.grip.kind != ClimbGripKind::Ladder {
            anchor.hand_world_at_grab += applied - requested;
            // A blocked pull made no body momentum. Do not turn that rejected
            // travel into a throw when the hand is deliberately opened.
            if let Some(travel) = self.recent_anchor_travel.back_mut() {
                *travel = -applied;
            }
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
        self.armed = [false, false];
        self.reset_grace();
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
            |point, _| Some(ladder_grip(point)),
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
            |_, _| None,
            |_| true,
        );
        assert_translation(pull, vec3(0.0, 0.4, 0.0));

        // Opening the hand drops the hold.
        let released = climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 0.0)],
            |_, _| None,
            |_| true,
        );
        assert_eq!(released.translation, None);
        assert_eq!(climb.anchor(), None);
        assert_eq!(climb.grips().count(), 0);
    }

    #[test]
    fn deck_transfer_requires_a_nearby_valid_hold_of_either_kind() {
        for kind in [ClimbGripKind::Ladder, ClimbGripKind::Ledge] {
            for (squeeze, empty, alive, distance, expected) in [
                (1.0, true, true, 0.5, true),
                (0.0, true, true, 0.5, false),
                (1.0, false, true, 0.5, false),
                (1.0, true, false, 0.5, false),
                (1.0, true, true, 1.6, false),
            ] {
                for held in 0..2 {
                    let mut climb = HandClimb::default();
                    let pawn = Vector3::zero();
                    let rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
                    let reach = vec3(0.0, 1.0, -1.0);
                    let mut hands = [no_hand(), no_hand()];
                    hands[held] = hand(reach, 1.0);
                    climb.update(
                        pawn,
                        rotation,
                        DT,
                        hands,
                        |p, _| {
                            Some(ClimbGrip {
                                kind,
                                entity_id: Some(shipyard::EntityId::from_inner(123).unwrap()),
                                ..ladder_grip(p)
                            })
                        },
                        |_| true,
                    );
                    let mut hands = [
                        hand(reach + vec3(distance, 0.0, 0.0), 1.0),
                        hand(reach + vec3(distance, 0.0, 0.0), 1.0),
                    ];
                    hands[held] = ClimbHandInput {
                        is_empty: empty,
                        ..hand(reach, squeeze)
                    };
                    climb.update(
                        pawn,
                        rotation,
                        DT,
                        hands,
                        |p, rule| {
                            assert_eq!(
                                rule == GripProbe::Transfer,
                                expected,
                                "{kind:?}, hand {held}"
                            );
                            (rule == GripProbe::Transfer).then_some(ClimbGrip {
                                kind: ClimbGripKind::Ledge,
                                normal: vec3(0.0, 1.0, 0.0),
                                ..ladder_grip(p)
                            })
                        },
                        |_| alive,
                    );
                    assert_eq!(climb.grips[1 - held].is_some(), expected);
                    if expected {
                        assert_eq!(climb.grips().count(), 2);
                        let mut hands = [
                            hand(reach + vec3(distance, 0.0, 0.0), 1.0),
                            hand(reach + vec3(distance, 0.0, 0.0), 1.0),
                        ];
                        hands[held] = no_hand();
                        let frame = climb.update(pawn, rotation, DT, hands, |_, _| None, |_| true);
                        assert_translation(frame, Vector3::zero());
                        assert!(frame.launch.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn deck_transfer_grace_requires_the_other_ladder_hold_to_remain_valid() {
        for break_hold in [false, true] {
            let mut climb = HandClimb::default();
            let pawn = Vector3::zero();
            let rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
            let reach = vec3(0.0, 1.0, -1.0);
            climb.update(
                pawn,
                rotation,
                DT,
                [hand(reach, 1.0), no_hand()],
                |p, _| Some(ladder_grip(p)),
                |_| true,
            );
            // Squeeze the free hand before contact.
            climb.update(
                pawn,
                rotation,
                DT,
                [hand(reach, 1.0), hand(reach, 1.0)],
                |_, _| None,
                |_| true,
            );
            let moved = reach + vec3(0.0, if break_hold { 1.0 } else { 0.0 }, 0.0);
            climb.update(
                pawn,
                rotation,
                DT,
                [hand(moved, 1.0), hand(reach, 1.0)],
                |p, rule| {
                    (rule == GripProbe::Transfer).then_some(ClimbGrip {
                        kind: ClimbGripKind::Ledge,
                        normal: vec3(0.0, 1.0, 0.0),
                        ..ladder_grip(p)
                    })
                },
                |_| true,
            );
            // Off the ladder hold the deck is a grace grab; with that hold
            // broken first there is nothing to grab from.
            assert_eq!(
                climb.grips[1].map(|anchor| (anchor.grip.kind, anchor.grace)),
                (!break_hold).then_some((ClimbGripKind::Surface, 1))
            );
        }
    }

    #[test]
    fn a_fresh_squeeze_grabs_on_contact_however_late_but_never_regrabs_after_a_break() {
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
                |_, _| None,
                |_| true,
            );
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p, _| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(climb.anchor().is_some());
        // Over-stretched off the hold, then back onto rungs, still squeezing:
        // the closed hand stays off until it is opened and pressed again.
        let broken = reach + vec3(0.0, 1.0, 0.0);
        for at in [broken, broken, reach, reach] {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 1.0)],
                |p, _| Some(ladder_grip(p)),
                |_| true,
            );
            assert!(climb.anchor().is_none());
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 0.0)],
            |_, _| None,
            |_| true,
        );
        // No acquisition timer: a hand closed long before contact still grabs.
        for _ in 0..120 {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(reach, 1.0)],
                |_, _| None,
                |_| true,
            );
        }
        climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p, _| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(
            climb.anchor().is_some(),
            "a closed empty hand grabs on contact"
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
                    |p, _| Some(ladder_grip(p)),
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
                    |p, _| Some(ladder_grip(p)),
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
            |p, _| Some(ladder_grip(p)),
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
            |_, _| None,
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
                    |_, _| None,
                    |_| true
                )
                .translation,
            None,
        );
    }

    #[test]
    fn a_blocked_deck_pull_keeps_support_when_the_ladder_hand_opens() {
        let mut climb = HandClimb::default();
        let pawn = Vector3::zero();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let ladder = vec3(-0.3, 1.0, -0.5);
        let deck = vec3(0.3, 1.2, -0.5);
        climb.update(
            pawn,
            identity,
            DT,
            [hand(ladder, 1.0), no_hand()],
            |p, _| Some(ladder_grip(p)),
            |_| true,
        );
        climb.update(
            pawn,
            identity,
            DT,
            [hand(ladder, 1.0), hand(deck, 1.0)],
            |p, _| {
                Some(ClimbGrip {
                    kind: ClimbGripKind::Ledge,
                    ..ladder_grip(p)
                })
            },
            |_| true,
        );
        // The wall prevents the pawn following this ordinary inward pull.
        // A still-closed deck hand must not silently lose its hold underneath
        // the ladder hand, leaving the next handoff unsupported.
        for distance in [0.2, 0.4, 0.6, 0.8] {
            let frame = climb.update(
                pawn,
                identity,
                DT,
                [
                    hand(ladder, 1.0),
                    hand(deck + vec3(0.0, 0.0, distance), 1.0),
                ],
                |_, _| None,
                |_| true,
            );
            assert!(frame.translation.is_some());
            assert!(frame.launch.is_none());
        }
        let pulled = deck + vec3(0.0, 0.0, 0.8);
        let handoff = climb.update(
            pawn,
            identity,
            DT,
            [hand(ladder, 0.0), hand(pulled, 1.0)],
            |_, _| None,
            |_| true,
        );
        assert_eq!(climb.anchor(), Some(Handedness::Right));
        // Taken off the ladder, the deck is a grace hold; it keeps a blocked
        // pull's support all the same.
        assert_eq!(
            climb.anchor_grip().map(|anchor| anchor.grip.kind),
            Some(ClimbGripKind::Surface)
        );
        assert!(handoff.translation.is_some());
        assert!(handoff.launch.is_none());
        // Relaxing the pull recovers the same anchored hold; it never rebases
        // the hand into space or ratchets the pawn through the obstruction.
        let relaxed = climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(deck, 1.0)],
            |_, _| None,
            |_| true,
        );
        assert_translation(relaxed, Vector3::zero());
        for _ in 0..RELEASE_SAMPLE_FRAMES {
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(deck, 1.0)],
                |_, _| None,
                |_| true,
            );
        }
        let released = climb.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(deck, 0.0)],
            |_, _| None,
            |_| true,
        );
        assert!(released.translation.is_none());
        assert!(released.launch.is_none());
        assert_eq!(climb.grips().count(), 0);
    }

    #[test]
    fn a_passive_ledge_cannot_support_a_handoff_after_the_body_moves_away() {
        for active in 0..2 {
            for active_kind in [ClimbGripKind::Ladder, ClimbGripKind::Ledge] {
                let mut climb = HandClimb::default();
                let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
                let positions = [vec3(-0.3, 1.0, -0.5), vec3(0.3, 1.0, -0.5)];
                let inputs = |active_squeeze, travel| {
                    std::array::from_fn(|i| {
                        hand(
                            positions[i] - if i == active { travel } else { Vector3::zero() },
                            if i == active { active_squeeze } else { 1.0 },
                        )
                    })
                };
                climb.update(
                    Vector3::zero(),
                    identity,
                    DT,
                    inputs(0.0, Vector3::zero()),
                    |p, _| {
                        Some(ClimbGrip {
                            kind: ClimbGripKind::Ledge,
                            ..ladder_grip(p)
                        })
                    },
                    |_| true,
                );
                climb.update(
                    Vector3::zero(),
                    identity,
                    DT,
                    inputs(1.0, Vector3::zero()),
                    |p, _| {
                        Some(ClimbGrip {
                            kind: active_kind,
                            ..ladder_grip(p)
                        })
                    },
                    |_| true,
                );
                let travel = vec3(0.0, 0.8, 0.0);
                climb.update(
                    travel,
                    identity,
                    DT,
                    inputs(1.0, travel),
                    |_, _| None,
                    |_| true,
                );
                assert_eq!(
                    climb.grips().count(),
                    1,
                    "abandoned passive ledge must break"
                );
                let released = climb.update(
                    travel,
                    identity,
                    DT,
                    inputs(0.0, travel),
                    |_, _| None,
                    |_| true,
                );
                assert!(
                    released.translation.is_none(),
                    "cannot hang from a distant stale ledge"
                );
                assert!(climb.anchor().is_none());
            }
        }
    }

    #[test]
    fn only_applied_ledge_pull_motion_becomes_a_release_throw() {
        for blocked in [false, true] {
            let mut climb = HandClimb::default();
            let mut pawn = Vector3::zero();
            let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
            let mut at = vec3(0.3, 1.2, -0.5);
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 1.0)],
                |p, _| {
                    Some(ClimbGrip {
                        kind: ClimbGripKind::Ledge,
                        ..ladder_grip(p)
                    })
                },
                |_| true,
            );
            for _ in 0..RELEASE_SAMPLE_FRAMES {
                at.y -= 0.1;
                let frame = climb.update(
                    pawn,
                    identity,
                    DT,
                    [no_hand(), hand(at, 1.0)],
                    |_, _| None,
                    |_| true,
                );
                let requested = frame.translation.unwrap();
                let applied = if blocked { Vector3::zero() } else { requested };
                climb.resolve_translation(requested, applied);
                pawn += applied;
            }
            // Opening can coincide with one more tracked pull. That frame
            // never moved the body and must not resurrect blocked momentum.
            at.y -= 0.2;
            let released = climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 0.0)],
                |_, _| None,
                |_| true,
            );
            assert!(released.translation.is_none());
            assert_eq!(
                released.launch.is_none(),
                blocked,
                "opening after a blocked pull drops; a real pull still throws"
            );
        }
    }

    #[test]
    fn a_held_ledge_still_releases_if_occupied_or_removed() {
        for occupied in [false, true] {
            let mut climb = HandClimb::default();
            let pawn = Vector3::zero();
            let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
            let at = vec3(0.3, 1.2, -0.5);
            climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), hand(at, 1.0)],
                |p, _| {
                    Some(ClimbGrip {
                        kind: ClimbGripKind::Ledge,
                        entity_id: Some(shipyard::EntityId::from_inner(123).unwrap()),
                        ..ladder_grip(p)
                    })
                },
                |_| true,
            );
            let mut holding = hand(at, 1.0);
            holding.is_empty = !occupied;
            let frame = climb.update(
                pawn,
                identity,
                DT,
                [no_hand(), holding],
                |_, _| None,
                |_| occupied,
            );
            assert!(frame.translation.is_none());
            assert!(frame.launch.is_none());
            assert!(!climb.holds_a_ledge());
        }
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
            |p, _| Some(ladder_grip(p)),
            |_| true,
        );
        climb.update(
            pawn,
            identity,
            DT,
            [hand(left, 1.0), hand(right, 1.0)],
            |p, _| Some(ladder_grip(p)),
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
            |_, _| None,
            |_| true,
        );

        // Letting the left go must not yank the body back to the right hand's
        // stale grab point: the handoff frame asks for no motion at all.
        let handoff = climb.update(
            pawn - travel,
            identity,
            DT,
            [hand(left + travel, 0.0), hand(right + travel, 1.0)],
            |_, _| None,
            |_| true,
        );
        assert_eq!(climb.anchor(), Some(Handedness::Right));
        assert_eq!(
            climb.anchor_grip().unwrap().pawn_at_grab,
            pawn - travel,
            "handoff starts a fresh deliberate pull, not the passive hold's old progress"
        );
        assert_translation(handoff, Vector3::zero());

        // ... and the right hand then drives from where it actually is.
        let after = climb.update(
            pawn - travel,
            identity,
            DT,
            [
                hand(left + travel, 0.0),
                hand(right + travel - vec3(0.0, 0.2, 0.0), 1.0),
            ],
            |_, _| None,
            |_| true,
        );
        assert_translation(after, vec3(0.0, 0.2, 0.0));
    }

    #[test]
    fn a_release_throws_the_body_against_the_pull() {
        // A hand hauled down 0.08 wu per frame throws the body up at 4.8 wu/s.
        let velocity = release_velocity(&[vec3(0.0, -0.08, 0.0); 4], DT)
            .expect("a real pull should throw the body");
        assert!(
            (velocity - vec3(0.0, 4.8, 0.0)).magnitude() < 1.0e-4,
            "{velocity:?}"
        );

        // The mean is what counts: one frame of tracking noise cannot double it.
        let averaged = release_velocity(
            &[
                vec3(0.0, -0.08, 0.0),
                vec3(0.0, -0.24, 0.0),
                vec3(0.0, -0.08, 0.0),
                vec3(0.0, 0.08, 0.0),
            ],
            DT,
        )
        .expect("a real pull should throw the body");
        assert!((averaged.y - 4.8).abs() < 1.0e-4, "{averaged:?}");
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
            |p, _| Some(ladder_grip(p)),
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
                |_, _| None,
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
            |_, _| None,
            |_| true,
        )
    }

    #[test]
    fn letting_go_of_the_last_hold_launches_the_body() {
        // A quick haul: 0.08 wu of hand travel per frame, let go mid-pull.
        let released = pull_and_finish(4, vec3(0.0, -0.08, 0.0), |climb, at| {
            open_the_hand(climb, at + vec3(0.0, -0.08, 0.0))
        });
        let launch = released.launch.expect("a quick pull should launch");
        assert!((launch.y - 4.8).abs() < 0.1, "{launch:?}");
        assert_eq!(released.translation, None);
        assert!(released.released);

        // Stopping the hand before opening it is part of the pull: the same
        // haul, released from rest, throws proportionally less.
        let stopped = pull_and_finish(4, vec3(0.0, -0.08, 0.0), open_the_hand);
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
                    |_, _| None,
                    |_| true,
                );
                broke
            },
        );
        assert_eq!(broke.translation, None, "the grip should have broken");
        assert_eq!(broke.launch, None);
        assert!(!broke.released, "a broken hold is not a release");
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
                |p, _| probe.then(|| ladder_grip(p)),
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
        assert!(!handoff.released, "a handoff is not a release");
        assert_eq!(climb.anchor(), Some(Handedness::Right));
    }

    #[test]
    fn a_squeeze_held_through_a_suspension_is_no_fresh_press() {
        let mut climb = HandClimb::default();
        let rung = vec3(0.0, 1.0, -1.0);
        let step = |climb: &mut HandClimb, squeeze: f32| {
            climb.update(
                Vector3::zero(),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                DT,
                [no_hand(), hand(rung, squeeze)],
                |p, _| Some(ladder_grip(p)),
                |_| true,
            )
        };
        step(&mut climb, 1.0);
        step(&mut climb, 0.0);
        assert_eq!(climb.grips().count(), 0);
        // Squeezed while an exit owns the body, and still squeezed after.
        climb.track_squeezes_only([0.0, 1.0]);
        step(&mut climb, 1.0);
        assert_eq!(climb.grips().count(), 0, "a held squeeze is no fresh press");
        step(&mut climb, 0.0);
        step(&mut climb, 1.0);
        assert_eq!(climb.grips().count(), 1, "a fresh press grabs");
    }

    #[test]
    fn opening_a_still_last_hand_is_a_release_and_predicts_its_throw() {
        // Hanging still: a release that throws nothing is still a release.
        let still = pull_and_finish(4, Vector3::zero(), open_the_hand);
        assert_eq!(still.launch, None);
        assert!(still.released);

        // Mid-pull, the predicted throw is the one opening on the next frame
        // of that pull gives.
        let mut predicted = None;
        let launched = pull_and_finish(4, vec3(0.0, -0.08, 0.0), |climb, at| {
            predicted = climb.predicted_release_velocity(DT);
            open_the_hand(climb, at + vec3(0.0, -0.08, 0.0))
        });
        assert_eq!(predicted, launched.launch);
        assert!(predicted.is_some());
    }

    #[test]
    fn the_eye_must_clear_the_lip_of_a_ledge_the_hand_is_lying_on() {
        // A deliberate pull with the eye over the lip: vault, including late grabs.
        assert!(vault_ready(6.2, 0.2, 6.0, ClimbGripKind::Ledge, true));
        // Still below it, or only just level with it: keep pulling.
        assert!(!vault_ready(5.9, 0.2, 6.0, ClimbGripKind::Ledge, true));
        assert!(!vault_ready(
            6.0 + VAULT_EYE_MARGIN,
            0.2,
            6.0,
            ClimbGripKind::Ledge,
            true
        ));
        // Never pulled at all: a standing player who grabs a chest-high crate
        // was already looking over it, and leaning on it is not a mantle.
        assert!(!vault_ready(6.2, 0.0, 6.0, ClimbGripKind::Ledge, true));
        // Hooked on the lip's vertical face, not lying on the top.
        assert!(!vault_ready(6.2, 0.2, 6.0, ClimbGripKind::Ledge, false));
        // A ladder rail is never a vault, however high the eye gets.
        assert!(!vault_ready(9.0, 0.2, 6.0, ClimbGripKind::Ladder, true));
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
        let mut rung = HandClimb::default();
        rung.update(
            pawn,
            identity,
            DT,
            [no_hand(), hand(reach, 1.0)],
            |p, _| Some(ladder_grip(p)),
            |_| true,
        );
        assert!(!rung.holds_a_ledge());

        // A ledge (not taken off a ladder) is: and the anchor records the
        // body pose that grab was made from - what tells a pull from a grab.
        climb.update(
            pawn,
            identity,
            DT,
            [hand(reach, 1.0), no_hand()],
            |p, _| Some(ledge(p)),
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
            [hand(reach, 1.0), no_hand()],
            |_, _| None,
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
                |p, _| probe.then(|| ladder_grip(p)),
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
            |point, _| {
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
                    |_, _| None,
                    |_| false
                )
                .translation,
            None,
        );
    }

    /// Grace-grab world: rungs at `x <= -0.3`; everything else is a plain
    /// surface, holdable only as a grace grab.
    fn grace_world(p: Vector3<f32>, rule: GripProbe) -> Option<ClimbGrip> {
        if p.x <= -0.3 {
            return Some(ladder_grip(p));
        }
        (rule == GripProbe::Surface).then_some(ClimbGrip {
            kind: ClimbGripKind::Surface,
            ..ladder_grip(p)
        })
    }

    const RUNG: Vector3<f32> = vec3(-0.4, 1.0, -0.5);
    const WALL: [Vector3<f32>; 3] = [
        vec3(0.3, 1.0, -0.5),
        vec3(0.6, 1.2, -0.5),
        vec3(0.9, 1.0, -0.5),
    ];
    const OPEN: f32 = 0.0;
    const SQUEEZE: f32 = 1.0;

    /// Hand `a`'s and the other hand's (position, squeeze) for one frame.
    fn grace_step(
        climb: &mut HandClimb,
        a: usize,
        hand_a: (Vector3<f32>, f32),
        other: (Vector3<f32>, f32),
    ) -> ClimbFrame {
        let mut hands = [other, other];
        hands[a] = hand_a;
        climb.update(
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            DT,
            hands.map(|(at, squeeze)| hand(at, squeeze)),
            grace_world,
            |_| true,
        )
    }

    fn grace_of(climb: &HandClimb, index: usize) -> Option<u8> {
        climb.grips[index].map(|anchor| anchor.grace)
    }

    /// A ladder left of x = -0.3; elsewhere a floor top the ledge and deck
    /// transfer probes take (as at a ladder's top), which no surface probe sees.
    fn deck_world(p: Vector3<f32>, rule: GripProbe) -> Option<ClimbGrip> {
        if p.x <= -0.3 {
            return Some(ladder_grip(p));
        }
        (rule != GripProbe::Surface).then_some(ClimbGrip {
            kind: ClimbGripKind::Ledge,
            normal: Vector3::unit_y(),
            ..ladder_grip(p)
        })
    }

    #[test]
    fn a_floor_taken_off_a_ladder_is_a_grace_grab_and_two_at_most() {
        let step =
            |climb: &mut HandClimb, left: (Vector3<f32>, f32), right: (Vector3<f32>, f32)| {
                climb.update(
                    Vector3::zero(),
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    DT,
                    [hand(left.0, left.1), hand(right.0, right.1)],
                    deck_world,
                    |_| true,
                )
            };
        let mut climb = HandClimb::default();
        step(&mut climb, (RUNG, SQUEEZE), (WALL[0], OPEN));
        step(&mut climb, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        let deck = climb.grips[1].expect("the floor in front holds");
        assert_eq!(
            deck.grip.kind,
            ClimbGripKind::Surface,
            "never a vaulting ledge"
        );
        assert_eq!(deck.grace, 1);
        // Off the ladder: the other hand onto the floor too, the second grace.
        step(&mut climb, (RUNG, OPEN), (WALL[0], SQUEEZE));
        step(&mut climb, (WALL[1], SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 0), Some(2));
        assert_eq!(climb.grips[0].unwrap().grip.kind, ClimbGripKind::Surface);
        // No third.
        step(&mut climb, (WALL[1], SQUEEZE), (WALL[0], OPEN));
        step(&mut climb, (WALL[1], SQUEEZE), (WALL[2], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), None, "no third floor grab");

        // The same floor, not reached from a ladder: an ordinary ledge.
        let mut climb = HandClimb::default();
        step(&mut climb, (WALL[0], OPEN), (WALL[0], SQUEEZE));
        let ledge = climb.grips[1].expect("an ordinary ledge");
        assert_eq!(ledge.grip.kind, ClimbGripKind::Ledge);
        assert_eq!(ledge.grace, 0);
    }

    #[test]
    fn opening_the_ladder_hand_as_the_other_takes_the_floor_is_still_a_grace_grab() {
        let step =
            |climb: &mut HandClimb, left: (Vector3<f32>, f32), right: (Vector3<f32>, f32)| {
                climb.update(
                    Vector3::zero(),
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    DT,
                    [hand(left.0, left.1), hand(right.0, right.1)],
                    deck_world,
                    |_| true,
                );
            };
        let mut climb = HandClimb::default();
        step(&mut climb, (RUNG, SQUEEZE), (WALL[0], OPEN));
        // The same frame: the ladder hand opens, the other closes on the floor.
        step(&mut climb, (RUNG, OPEN), (WALL[0], SQUEEZE));
        let deck = climb.grips[1].expect("the floor holds");
        assert_eq!(
            (deck.grip.kind, deck.grace),
            (ClimbGripKind::Surface, 1),
            "never an ordinary vaulting ledge"
        );

        // Off the last grace hold in the same frame: no ordinary ledge, and
        // no grab past the allowance.
        step(&mut climb, (WALL[1], SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 0), Some(2));
        step(&mut climb, (WALL[1], SQUEEZE), (WALL[0], OPEN));
        step(&mut climb, (WALL[1], OPEN), (WALL[2], SQUEEZE));
        assert!(climb.grips[1].is_none(), "no third, nor an ordinary ledge");
    }

    #[test]
    fn a_hold_torn_away_as_the_other_hand_catches_a_ledge_carries_no_departure() {
        let mut climb = HandClimb::default();
        let step =
            |climb: &mut HandClimb, left: (Vector3<f32>, f32), right: (Vector3<f32>, f32)| {
                climb.update(
                    Vector3::zero(),
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    DT,
                    [hand(left.0, left.1), hand(right.0, right.1)],
                    deck_world,
                    |_| true,
                );
            };
        step(&mut climb, (RUNG, SQUEEZE), (WALL[0], OPEN));
        // The ladder hold breaks (the hand is dragged past the stretch limit)
        // the same frame the other hand closes on the floor: a catch, an
        // ordinary ledge.
        step(
            &mut climb,
            (RUNG + vec3(0.0, 1.0, 0.0), SQUEEZE),
            (WALL[0], SQUEEZE),
        );
        let caught = climb.grips[1].expect("the floor holds");
        assert_eq!((caught.grip.kind, caught.grace), (ClimbGripKind::Ledge, 0));
    }

    #[test]
    fn a_ladder_allows_two_grace_grabs_on_any_surface_and_never_a_third() {
        for a in 0..2 {
            let b = 1 - a;
            let mut climb = HandClimb::default();
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], OPEN));
            assert_eq!(grace_of(&climb, a), Some(0));
            assert_eq!(climb.grace_remaining(), 2);

            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1), "hand {a}: first grace grab");
            assert_eq!(climb.grips[b].unwrap().grip.kind, ClimbGripKind::Surface);
            assert_eq!(climb.grace_remaining(), 1);

            // Off the ladder, hanging from the wall: one more surface.
            grace_step(&mut climb, a, (RUNG, OPEN), (WALL[0], SQUEEZE));
            assert_eq!(climb.anchor(), Some(HANDS[b]));
            grace_step(&mut climb, a, (WALL[1], SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, a), Some(2), "hand {a}: second grace grab");
            assert_eq!(climb.grace_remaining(), 0);

            // Grace hold 2 authorizes nothing, however long the hand stays closed.
            grace_step(&mut climb, a, (WALL[1], SQUEEZE), (WALL[0], OPEN));
            assert_eq!(climb.anchor(), Some(HANDS[a]));
            for _ in 0..30 {
                grace_step(&mut climb, a, (WALL[1], SQUEEZE), (WALL[2], SQUEEZE));
                assert_eq!(grace_of(&climb, b), None, "hand {a}: no third grace grab");
                assert!(climb.probe_grip(HANDS[b], WALL[2], grace_world).is_none());
            }
            // A ladder is still an ordinary hold, and starts a new departure.
            grace_step(&mut climb, a, (WALL[1], SQUEEZE), (RUNG, OPEN));
            grace_step(&mut climb, a, (WALL[1], SQUEEZE), (RUNG, SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(0));
            assert_eq!(climb.grace_remaining(), 2);
        }
    }

    #[test]
    fn a_held_ladder_allows_one_free_regrab_then_none_until_grabbed_afresh() {
        for a in 0..2 {
            let b = 1 - a;
            let mut climb = HandClimb::default();
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], OPEN));
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1));
            assert_eq!(climb.grace_remaining(), 1);
            // One re-grab is free while the other hand is still on the ladder.
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], OPEN));
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[1], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1), "hand {a}: the free re-grab");
            assert_eq!(climb.grace_remaining(), 1, "it spends no grace");
            // A second is not.
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[1], OPEN));
            assert_eq!(
                climb.grace_remaining(),
                0,
                "nothing left to reach grace 2 from"
            );
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(
                grace_of(&climb, b),
                None,
                "hand {a}: no second free re-grab"
            );
            // A fresh ladder grab (by either hand) restores the allowance.
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (RUNG, OPEN));
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (RUNG, SQUEEZE));
            assert_eq!(climb.grace_remaining(), 2);
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], OPEN));
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1));
        }
    }

    #[test]
    fn the_free_regrab_needs_a_hand_still_on_the_ladder() {
        for a in 0..2 {
            let b = 1 - a;
            let step =
                |climb: &mut HandClimb, hand_a: (Vector3<f32>, f32), other: (Vector3<f32>, f32)| {
                    let mut hands = [other, other];
                    hands[a] = hand_a;
                    climb.update(
                        Vector3::zero(),
                        Quaternion::new(1.0, 0.0, 0.0, 0.0),
                        DT,
                        hands.map(|(at, squeeze)| hand(at, squeeze)),
                        deck_world,
                        |_| true,
                    );
                };
            let mut climb = HandClimb::default();
            step(&mut climb, (RUNG, SQUEEZE), (WALL[0], OPEN));
            step(&mut climb, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1));
            step(&mut climb, (RUNG, SQUEEZE), (WALL[0], OPEN));
            // The ladder hand opens the same frame the other re-grabs the floor.
            step(&mut climb, (RUNG, OPEN), (WALL[0], SQUEEZE));
            assert_eq!(
                grace_of(&climb, b),
                None,
                "hand {a}: no free re-grab off the ladder"
            );
        }
    }

    #[test]
    fn a_free_regrab_that_closes_on_an_item_refunds_the_regrab_not_grace() {
        let mut climb = HandClimb::default();
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), Some(1), "the free re-grab");
        // That squeeze closes on an item instead: the re-grab comes back,
        // grace 1 stays spent.
        climb.update(
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            DT,
            [
                hand(RUNG, SQUEEZE),
                ClimbHandInput {
                    is_empty: false,
                    ..hand(WALL[0], SQUEEZE)
                },
            ],
            grace_world,
            |_| true,
        );
        assert_eq!(grace_of(&climb, 1), None);
        assert_eq!(climb.grace_remaining(), 1);
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), Some(1), "the re-grab, again");
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), None, "and no more");
    }

    #[test]
    fn after_the_free_regrab_the_ladder_hand_takes_the_last_grace_grab() {
        let mut climb = HandClimb::default();
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), Some(1));
        // The ladder hand leaves for a surface: grace 2, the last.
        grace_step(&mut climb, 0, (RUNG, OPEN), (WALL[0], SQUEEZE));
        grace_step(&mut climb, 0, (WALL[1], SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 0), Some(2));
        assert_eq!(climb.grace_remaining(), 0);
        grace_step(&mut climb, 0, (WALL[1], SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (WALL[1], SQUEEZE), (WALL[2], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), None, "no third");
    }

    #[test]
    fn failed_squeezes_and_ordinary_holds_spend_no_grace() {
        let mut climb = HandClimb::default();
        let far = RUNG + vec3(1.6, 0.0, 0.0);
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (far, OPEN));
        // Beyond reach of the ladder hold: nothing, repeatedly.
        for squeeze in [SQUEEZE, OPEN, SQUEEZE, OPEN] {
            grace_step(&mut climb, 0, (RUNG, SQUEEZE), (far, squeeze));
        }
        assert_eq!(climb.grips().count(), 1);
        assert_eq!(climb.grace_remaining(), 2);
        // A second rung is an ordinary hold.
        grace_step(
            &mut climb,
            0,
            (RUNG, SQUEEZE),
            (RUNG + vec3(0.0, 0.3, 0.0), SQUEEZE),
        );
        assert_eq!(grace_of(&climb, 1), Some(0));
        assert_eq!(climb.grace_remaining(), 2);

        // An ordinary ledge hold (not off a ladder) is no ladder: a deck
        // transfer from it is ordinary, and it authorizes no grace grab.
        let ledges = |p: Vector3<f32>, rule: GripProbe| {
            (rule != GripProbe::Surface).then_some(ClimbGrip {
                kind: ClimbGripKind::Ledge,
                ..ladder_grip(p)
            })
        };
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let mut climb = HandClimb::default();
        let step =
            |climb: &mut HandClimb,
             hands: [ClimbHandInput; 2],
             probe: &dyn Fn(Vector3<f32>, GripProbe) -> Option<ClimbGrip>| {
                climb.update(Vector3::zero(), identity, DT, hands, probe, |_| true);
            };
        step(
            &mut climb,
            [hand(WALL[1], SQUEEZE), hand(WALL[0], OPEN)],
            &ledges,
        );
        step(
            &mut climb,
            [hand(WALL[1], SQUEEZE), hand(WALL[0], SQUEEZE)],
            &ledges,
        );
        assert_eq!(
            climb.grips[1].map(|a| (a.grip.kind, a.grace)),
            Some((ClimbGripKind::Ledge, 0))
        );
        step(
            &mut climb,
            [hand(WALL[1], SQUEEZE), hand(WALL[0], OPEN)],
            &grace_world,
        );
        step(
            &mut climb,
            [hand(WALL[1], SQUEEZE), hand(WALL[2], SQUEEZE)],
            &grace_world,
        );
        assert_eq!(grace_of(&climb, 1), None);
        assert_eq!(climb.grace_remaining(), 2);
    }

    #[test]
    fn a_hold_taken_this_frame_authorizes_no_grace_grab_until_the_next() {
        for a in 0..2 {
            let b = 1 - a;
            let mut climb = HandClimb::default();
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, a), Some(0));
            assert_eq!(grace_of(&climb, b), None, "hand {a}: same-frame support");
            // Still closed and armed: it takes the grace grab next frame.
            grace_step(&mut climb, a, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(grace_of(&climb, b), Some(1));
        }
    }

    #[test]
    fn grace_resets_on_landing_losing_every_hold_or_a_vault() {
        let exhaust = |climb: &mut HandClimb| {
            grace_step(climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
            grace_step(climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            grace_step(climb, 0, (RUNG, OPEN), (WALL[0], SQUEEZE));
            grace_step(climb, 0, (WALL[1], SQUEEZE), (WALL[0], SQUEEZE));
            assert_eq!(climb.grace_remaining(), 0);
        };
        let mut climb = HandClimb::default();
        exhaust(&mut climb);
        climb.set_grounded(true);
        assert_eq!(climb.grace_remaining(), 2);

        let mut climb = HandClimb::default();
        exhaust(&mut climb);
        grace_step(&mut climb, 0, (WALL[1], OPEN), (WALL[0], OPEN));
        assert_eq!(climb.grips().count(), 0);
        assert_eq!(climb.grace_remaining(), 2);

        let mut climb = HandClimb::default();
        exhaust(&mut climb);
        climb.release_all();
        assert_eq!(climb.grace_remaining(), 2);
    }

    #[test]
    fn a_ladder_departure_is_a_ladder_or_grace_hold_without_a_ledge() {
        let mut climb = HandClimb::default();
        assert!(!climb.on_ladder_departure());
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        // Off the ladder, on the grace hold alone: still a departure.
        grace_step(&mut climb, 0, (RUNG, OPEN), (WALL[0], SQUEEZE));
        assert!(climb.on_ladder_departure());
        grace_step(&mut climb, 0, (WALL[1], OPEN), (WALL[0], OPEN));
        assert!(
            !climb.on_ladder_departure(),
            "letting go of everything ends it"
        );

        // An ordinary ledge climb is none.
        let mut climb = HandClimb::default();
        let ledge = |p: Vector3<f32>, _| {
            Some(ClimbGrip {
                kind: ClimbGripKind::Ledge,
                ..ladder_grip(p)
            })
        };
        climb.update(
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            DT,
            [no_hand(), hand(vec3(0.0, 1.5, -0.5), SQUEEZE)],
            ledge,
            |_| true,
        );
        assert_eq!(climb.grips().count(), 1);
        assert!(!climb.on_ladder_departure());
    }

    #[test]
    fn only_landing_resets_grace_not_standing_at_the_ladder_foot() {
        let mut climb = HandClimb::default();
        climb.set_grounded(true);
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        climb.set_grounded(true);
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), Some(1));
        for grounded in [true, true, false, false] {
            climb.set_grounded(grounded);
            grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        }
        climb.set_grounded(false);
        // Its one free re-grab, then no more: standing at the foot did not
        // reset the allowance.
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), Some(1));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(
            grace_of(&climb, 1),
            None,
            "the held ladder re-minted grace 1"
        );
        climb.set_grounded(true);
        assert_eq!(climb.grace_remaining(), 2, "landing resets");
    }

    #[test]
    fn a_squeeze_that_closes_on_an_item_refunds_its_grace_grab() {
        for frames_held in [0, 1] {
            let mut climb = HandClimb::default();
            grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
            grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            for _ in 0..frames_held {
                grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
            }
            // The hand update took an item with the same squeeze.
            let mut full = hand(WALL[0], SQUEEZE);
            full.is_empty = false;
            climb.update(
                Vector3::zero(),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                DT,
                [hand(RUNG, SQUEEZE), full],
                grace_world,
                |_| true,
            );
            assert_eq!(grace_of(&climb, 1), None);
            let refunded = frames_held == 0;
            assert_eq!(climb.grace_remaining(), if refunded { 2 } else { 1 });
        }
    }

    #[test]
    fn the_glow_rule_is_the_acquisition_rule() {
        let mut climb = HandClimb::default();
        assert!(climb.probe_grip(HANDS[1], WALL[0], grace_world).is_none());
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        let offered = climb.probe_grip(HANDS[1], WALL[0], grace_world);
        assert_eq!(
            offered.map(|(grip, grace)| (grip.kind, grace)),
            Some((ClimbGripKind::Surface, 1))
        );
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        assert_eq!(grace_of(&climb, 1), offered.map(|(_, grace)| grace));

        // Torn off the ladder and still closed: no glow where it cannot grab.
        let mut climb = HandClimb::default();
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(
            &mut climb,
            0,
            (RUNG + vec3(0.0, -1.0, 0.0), SQUEEZE),
            (WALL[0], OPEN),
        );
        assert_eq!(climb.grips().count(), 0);
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        assert!(climb.probe_grip(HANDS[0], RUNG, grace_world).is_none());
        assert!(climb.probe_grip(HANDS[1], RUNG, grace_world).is_some());
    }

    #[test]
    fn a_surface_hold_never_vaults_or_balls_up_and_keeps_a_blocked_pull() {
        assert!(!vault_ready(10.0, 1.0, 0.0, ClimbGripKind::Surface, true));
        let mut climb = HandClimb::default();
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], OPEN));
        grace_step(&mut climb, 0, (RUNG, SQUEEZE), (WALL[0], SQUEEZE));
        grace_step(&mut climb, 0, (RUNG, OPEN), (WALL[0], SQUEEZE));
        assert!(!climb.holds_a_ledge());
        // The wall stops the body; the hold consumes the rejected travel
        // instead of stretching until it tears off.
        for pulled in [0.2, 0.4, 0.6, 0.8] {
            let frame = grace_step(
                &mut climb,
                0,
                (RUNG, OPEN),
                (WALL[0] + vec3(0.0, -pulled, 0.0), SQUEEZE),
            );
            let requested = frame.translation.expect("still held");
            climb.resolve_translation(requested, Vector3::zero());
        }
        assert_eq!(grace_of(&climb, 1), Some(1));
    }
}
