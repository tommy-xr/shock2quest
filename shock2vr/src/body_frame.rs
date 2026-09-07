//! Body-relative anchors: where the player's belt and shoulders are, so a hand
//! can reach them.
//!
//! Every persistent VR interaction that is not attached to a world object hangs
//! off one of these - the belt card, the shoulders that stand in for the
//! backpack. They are plain world-space points the existing hand-proximity code
//! already knows how to test, resolved once per frame from the tracked head.
//!
//! The head is the only tracked part of the body we have, so the frame is
//! derived from it: horizontal position from the head's floor projection,
//! heights from the tracked eye, facing from a **low-passed** head yaw. The
//! low-pass is the whole point - a body whose belt swung with every glance
//! would put the card somewhere new each time the player looked at it. Pitch
//! and roll are ignored outright: leaning over does not move a real belt
//! sideways.

use cgmath::{InnerSpace, Quaternion, Rad, Rotation, Rotation3, Vector3, vec3};

use crate::{METERS_PER_WORLD_UNIT, vr_config::Handedness};

/// Convert a real-world measurement (the units body dimensions are naturally
/// written in) into world units.
const fn meters(m: f32) -> f32 {
    m / METERS_PER_WORLD_UNIT
}

/// Belt height as a fraction of the tracked eye height. ~0.55 puts it at the
/// top of the hip on an average adult, which is where a real belt sits and
/// where the hand naturally falls when the arm hangs and the elbow bends.
const BELT_HEIGHT_FRACTION: f32 = 0.55;

/// How far the shoulder sockets sit below the eye.
const SHOULDER_DROP: f32 = meters(0.2);

/// Half the shoulder/hip width: how far each anchor sits to the side of the
/// body's midline.
const LATERAL_OFFSET: f32 = meters(0.18);

/// Reach radius of each anchor. The belt card is a small object the hand has to
/// find; a shoulder is a "throw it back there" gesture and is deliberately more
/// forgiving.
const BELT_RADIUS: f32 = meters(0.09);
const SHOULDER_RADIUS: f32 = meters(0.15);

/// Minimum vertical gap between the belt and the shoulders, so a deep crouch
/// (which lowers the eye, and with it both anchors) can never bring the two
/// zones into contact and make "which anchor is this" a matter of rounding.
const ANCHOR_SEPARATION: f32 = BELT_RADIUS + SHOULDER_RADIUS + meters(0.05);

/// Time constant of the yaw low-pass, in seconds. Long enough that a glance
/// leaves the body where it was, short enough that turning to walk brings the
/// belt round with you well before you reach for it.
const YAW_TIME_CONSTANT_SECS: f32 = 0.5;

/// A place on the player's body a hand can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyAnchor {
    /// Left hip: the belt card.
    Belt,
    /// Either shoulder: the backpack (stow what you hold, draw what you stowed).
    Shoulder(Handedness),
}

/// This frame's resolved body anchors, in world space.
#[derive(Clone, Copy, Debug)]
pub struct BodyFrame {
    belt: Vector3<f32>,
    /// Indexed by [`crate::vr_config::hand_slot`].
    shoulders: [Vector3<f32>; 2],
    rotation: Quaternion<f32>,
}

impl BodyFrame {
    /// Where the belt card sits.
    pub fn belt(&self) -> Vector3<f32> {
        self.belt
    }

    /// Where one shoulder socket sits.
    pub fn shoulder(&self, hand: Handedness) -> Vector3<f32> {
        self.shoulders[crate::vr_config::hand_slot(hand)]
    }

    /// The body's facing - yaw only, so anything parented to it stays upright.
    pub fn rotation(&self) -> Quaternion<f32> {
        self.rotation
    }

    /// Which anchor, if any, a hand at `position` is inside. The belt is tested
    /// first: it is the smaller, more precisely-placed zone, and
    /// [`ANCHOR_SEPARATION`] keeps the two from ever overlapping anyway.
    pub fn anchor_at(&self, position: Vector3<f32>) -> Option<BodyAnchor> {
        if (position - self.belt).magnitude2() <= BELT_RADIUS * BELT_RADIUS {
            return Some(BodyAnchor::Belt);
        }
        for hand in [Handedness::Left, Handedness::Right] {
            if (position - self.shoulder(hand)).magnitude2() <= SHOULDER_RADIUS * SHOULDER_RADIUS {
                return Some(BodyAnchor::Shoulder(hand));
            }
        }
        None
    }
}

/// Per-frame inputs the frame is derived from.
pub struct BodyFrameInput {
    /// The pawn (player collider) origin and facing, in world space.
    pub pawn_position: Vector3<f32>,
    pub pawn_rotation: Quaternion<f32>,
    /// The tracked head, in pawn space - the same space
    /// [`crate::input_context::Hand::position`] uses.
    pub head_position: Vector3<f32>,
    pub head_rotation: Quaternion<f32>,
    /// How far the pawn origin sits above the floor, world units
    /// (`physics::player_center_above_floor`). Crouching shrinks it, which is
    /// what lowers the anchors with the player.
    pub pawn_above_floor: f32,
    /// The fixed simulation step this frame integrates, for the yaw low-pass.
    pub dt: f32,
}

/// Carries the low-passed yaw between frames. Runtime-only: the frame is
/// re-derived from the live head pose, so a restored save simply starts from
/// wherever the player is looking.
#[derive(Clone, Debug, Default)]
pub struct BodyFrameTracker {
    /// Pawn-relative head yaw, radians. `None` until the first tracked frame,
    /// so the body starts already facing the head instead of easing round to it.
    yaw: Option<f32>,
}

impl BodyFrameTracker {
    /// Advance the low-pass and resolve this frame's anchors.
    pub fn update(&mut self, input: &BodyFrameInput) -> BodyFrame {
        // An untracked head arrives as the zero quaternion, which cgmath
        // rotates by silently (see the VR UI rules): treat it as "no new
        // reading" and keep the yaw we had rather than snapping the body to
        // pawn-forward.
        if let Some(observed) = head_yaw(input.head_rotation) {
            self.yaw = Some(match self.yaw {
                None => observed,
                Some(current) => {
                    let alpha = 1.0 - (-input.dt / YAW_TIME_CONSTANT_SECS).exp();
                    wrap_pi(current + wrap_pi(observed - current) * alpha)
                }
            });
        }
        let rotation = input.pawn_rotation * Quaternion::from_angle_y(Rad(self.yaw.unwrap_or(0.0)));
        let right = rotation.rotate_vector(vec3(1.0, 0.0, 0.0));

        // Horizontal placement follows the head (lean out over a railing and
        // your belt goes with you); heights come off the tracked eye.
        let floor_y = input.pawn_position.y - input.pawn_above_floor;
        let ground = input.pawn_position
            + input.pawn_rotation.rotate_vector(vec3(
                input.head_position.x,
                0.0,
                input.head_position.z,
            ));
        let eye_above_floor = input.head_position.y + input.pawn_above_floor;

        let shoulder_y = floor_y + eye_above_floor - SHOULDER_DROP;
        let belt_y =
            (floor_y + eye_above_floor * BELT_HEIGHT_FRACTION).min(shoulder_y - ANCHOR_SEPARATION);
        let at = |y: f32| vec3(ground.x, y, ground.z);

        BodyFrame {
            // The card rides the left hip.
            belt: at(belt_y) - right * LATERAL_OFFSET,
            shoulders: [
                at(shoulder_y) - right * LATERAL_OFFSET,
                at(shoulder_y) + right * LATERAL_OFFSET,
            ],
            rotation,
        }
    }
}

/// How long a hand keeps the anchor it was over after losing it, in fixed
/// 60 Hz frames (~200 ms). A hand reaching behind the shoulder is exactly where
/// tracking is worst, and a pose that drops out for a frame must not read as
/// "left the zone" - the release it is about to make would then land on the
/// floor instead of in the backpack.
const ANCHOR_HOLD_FRAMES: u8 = 12;

/// What a hand's body-anchor gesture resolved to this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorGesture {
    /// Opened over a shoulder holding something: it goes in the backpack.
    Stow(Handedness),
    /// Gripped at a shoulder with an empty hand: draw the last weapon stowed.
    Draw(Handedness),
    /// Gripped at the belt with an empty hand: take the card.
    TakeCard(Handedness),
    /// Opened while holding the card: it goes back on the belt, wherever the
    /// hand is - the card is never dropped in the world.
    ReturnCard(Handedness),
}

/// What one hand is doing this frame, for [`AnchorGestures::update`].
#[derive(Clone, Copy, Debug)]
pub struct HandAnchorInput {
    pub hand: Handedness,
    /// Raw grip value, thresholded here so the edge is defined in one place.
    pub squeeze: f32,
    /// The anchor the hand is inside right now, from [`BodyFrame::anchor_at`].
    pub anchor: Option<BodyAnchor>,
    /// Whether the hand holds a world pickup.
    pub holding: bool,
    /// Whether the hand holds the belt card.
    pub holds_card: bool,
    /// Whether the backpack could take what the hand holds.
    pub can_stow: bool,
    /// Whether there is a stowed weapon left to draw.
    pub can_draw: bool,
    /// Whether the player has collected any credential, i.e. whether the belt
    /// card exists at all.
    pub has_card: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct HandAnchorState {
    squeezing: bool,
    anchor: Option<BodyAnchor>,
    hold_frames: u8,
}

/// The per-hand body-anchor gesture state machine: which anchor a hand is at
/// (with hysteresis), and what its grip edges mean there.
#[derive(Clone, Debug, Default)]
pub struct AnchorGestures {
    /// Indexed by [`crate::vr_config::hand_slot`].
    hands: [HandAnchorState; 2],
}

impl AnchorGestures {
    /// Advance one hand a frame. Returns the gesture its grip edge committed
    /// to, if any, and leaves [`Self::affordance`] describing what the hand
    /// could do next.
    pub fn update(&mut self, input: &HandAnchorInput) -> Option<AnchorGesture> {
        let state = &mut self.hands[crate::vr_config::hand_slot(input.hand)];
        // Decide on this frame's anchor first, then expire: the frame the
        // window runs out is still one the hand gets to act on.
        let anchor = input.anchor.or(state.anchor);
        match input.anchor {
            Some(anchor) => {
                state.anchor = Some(anchor);
                state.hold_frames = ANCHOR_HOLD_FRAMES;
            }
            None if state.hold_frames > 0 => state.hold_frames -= 1,
            None => state.anchor = None,
        }

        let squeezing = input.squeeze > crate::ui::VR_TRIGGER_THRESHOLD;
        let was_squeezing = std::mem::replace(&mut state.squeezing, squeezing);

        // Only a real grip edge commits. Losing the pose behind the shoulder
        // changes nothing: the controller keeps reporting its grip.
        if !was_squeezing && squeezing {
            return match anchor {
                Some(BodyAnchor::Shoulder(_)) if !input.holding && !input.holds_card => {
                    input.can_draw.then_some(AnchorGesture::Draw(input.hand))
                }
                Some(BodyAnchor::Belt) if !input.holding && !input.holds_card => input
                    .has_card
                    .then_some(AnchorGesture::TakeCard(input.hand)),
                _ => None,
            };
        }
        if was_squeezing && !squeezing {
            if input.holds_card {
                return Some(AnchorGesture::ReturnCard(input.hand));
            }
            if input.holding && input.can_stow {
                if let Some(BodyAnchor::Shoulder(_)) = anchor {
                    return Some(AnchorGesture::Stow(input.hand));
                }
            }
        }
        None
    }

    /// What the hand's anchor offers right now: green when the gesture would
    /// go through, amber when the anchor is recognised but the action is not
    /// available (a full backpack, nothing stowed to draw). `None` leaves the
    /// light to the hand's own raycast.
    pub fn affordance(
        &self,
        input: &HandAnchorInput,
    ) -> Option<crate::hand_affordance::HandAffordance> {
        use crate::hand_affordance::HandAffordance;

        let state = &self.hands[crate::vr_config::hand_slot(input.hand)];
        let eligible = |yes: bool| {
            Some(if yes {
                HandAffordance::Grabbable
            } else {
                HandAffordance::Blocked
            })
        };
        match state.anchor? {
            BodyAnchor::Shoulder(_) if input.holding => eligible(input.can_stow),
            BodyAnchor::Shoulder(_) if !input.holds_card => eligible(input.can_draw),
            BodyAnchor::Belt if !input.holding && !input.holds_card => eligible(input.has_card),
            _ => None,
        }
    }
}

/// The yaw a rotation faces, radians about +Y, or `None` for an untracked
/// (zero-magnitude) pose.
fn head_yaw(rotation: Quaternion<f32>) -> Option<f32> {
    if rotation.magnitude2() < 1e-6 {
        return None;
    }
    // `Quaternion::from_angle_y(t) * (0, 0, -1)` is `(-sin t, 0, -cos t)`.
    let forward = rotation.normalize().rotate_vector(vec3(0.0, 0.0, -1.0));
    Some((-forward.x).atan2(-forward.z))
}

/// Fold an angle into `[-pi, pi]`, so smoothing takes the short way round.
fn wrap_pi(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let wrapped = (angle + std::f32::consts::PI).rem_euclid(tau);
    wrapped - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use cgmath::{Deg, Zero};

    use super::*;

    const STEP: f32 = 1.0 / 60.0;

    /// A standing player, facing pawn-forward, at the origin.
    fn standing(head_rotation: Quaternion<f32>) -> BodyFrameInput {
        BodyFrameInput {
            pawn_position: Vector3::zero(),
            pawn_rotation: Quaternion::from_angle_y(Deg(0.0)),
            head_position: vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
            head_rotation,
            pawn_above_floor: crate::physics::player_center_above_floor(false),
            dt: STEP,
        }
    }

    /// The first tracked frame places the body where the head already is - no
    /// easing in from pawn-forward.
    #[test]
    fn the_first_frame_adopts_the_head_yaw() {
        let mut tracker = BodyFrameTracker::default();
        let frame = tracker.update(&standing(Quaternion::from_angle_y(Deg(90.0))));

        let forward = frame.rotation().rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!(
            forward.x < -0.99,
            "a 90 deg head should face -X immediately, got {forward:?}"
        );
    }

    /// The low-pass is the feature: a head that snaps 90 degrees leaves the
    /// belt where it was for the frame, and takes about the time constant to
    /// bring it most of the way round.
    #[test]
    fn a_head_turn_drags_the_body_round_over_the_time_constant() {
        let mut tracker = BodyFrameTracker::default();
        tracker.update(&standing(Quaternion::from_angle_y(Deg(0.0))));
        let turned = || standing(Quaternion::from_angle_y(Deg(90.0)));

        let after_one_frame = tracker.update(&turned());
        let yaw = |frame: &BodyFrame| {
            let f = frame.rotation().rotate_vector(vec3(0.0, 0.0, -1.0));
            (-f.x).atan2(-f.z).to_degrees()
        };
        assert!(
            yaw(&after_one_frame) < 5.0,
            "one frame should barely move the body, got {}",
            yaw(&after_one_frame)
        );

        let mut frame = after_one_frame;
        for _ in 1..(YAW_TIME_CONSTANT_SECS / STEP) as usize {
            frame = tracker.update(&turned());
        }
        // One time constant is 1 - 1/e of the way there.
        assert!(
            (55.0..75.0).contains(&yaw(&frame)),
            "one time constant should be ~63% of the turn, got {}",
            yaw(&frame)
        );
    }

    /// Smoothing takes the short way round the wrap, rather than unwinding
    /// almost a full turn the long way.
    #[test]
    fn yaw_smoothing_crosses_the_wrap_the_short_way() {
        let mut tracker = BodyFrameTracker::default();
        tracker.update(&standing(Quaternion::from_angle_y(Deg(175.0))));
        let frame = tracker.update(&standing(Quaternion::from_angle_y(Deg(-175.0))));

        let f = frame.rotation().rotate_vector(vec3(0.0, 0.0, -1.0));
        let yaw = (-f.x).atan2(-f.z).to_degrees();
        assert!(
            yaw.abs() > 170.0,
            "the body should stay near the wrap, not swing through 0, got {yaw}"
        );
    }

    /// An untracked head (the zero quaternion) is not a reading. Without the
    /// guard, cgmath returns the vector unrotated and the body snaps to
    /// pawn-forward.
    #[test]
    fn an_untracked_head_holds_the_last_yaw() {
        let mut tracker = BodyFrameTracker::default();
        tracker.update(&standing(Quaternion::from_angle_y(Deg(90.0))));

        let mut untracked = standing(Quaternion::from_angle_y(Deg(90.0)));
        untracked.head_rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        let frame = tracker.update(&untracked);

        let forward = frame.rotation().rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!(
            forward.x < -0.99,
            "an untracked frame must not swing the body, got {forward:?}"
        );
    }

    /// Crouching lowers the tracked eye, and the belt and shoulders come down
    /// with it.
    #[test]
    fn crouching_lowers_the_belt_and_the_shoulders() {
        let mut standing_tracker = BodyFrameTracker::default();
        let upright = standing_tracker.update(&standing(Quaternion::from_angle_y(Deg(0.0))));

        let mut crouch = standing(Quaternion::from_angle_y(Deg(0.0)));
        crouch.head_position.y = crate::PLAYER_CROUCH_EYE_HEIGHT / dark::SCALE_FACTOR;
        crouch.pawn_above_floor = crate::physics::player_center_above_floor(true);
        let crouched = BodyFrameTracker::default().update(&crouch);

        assert!(
            crouched.belt().y < upright.belt().y,
            "crouched belt {} should be below standing {}",
            crouched.belt().y,
            upright.belt().y
        );
        assert!(
            crouched.shoulder(Handedness::Left).y < upright.shoulder(Handedness::Left).y,
            "crouched shoulders should be below standing ones"
        );
    }

    /// The three zones never overlap, standing or crouched - a hand is only
    /// ever in one of them, so the gesture it commits to is never a matter of
    /// test order.
    #[test]
    fn the_anchor_zones_stay_disjoint_at_every_stance() {
        for eye in [0.5, 1.04, 1.4] {
            let mut input = standing(Quaternion::from_angle_y(Deg(0.0)));
            input.head_position.y = eye;
            let frame = BodyFrameTracker::default().update(&input);

            let centers = [
                (frame.belt(), BELT_RADIUS),
                (frame.shoulder(Handedness::Left), SHOULDER_RADIUS),
                (frame.shoulder(Handedness::Right), SHOULDER_RADIUS),
            ];
            for (i, (a, ra)) in centers.iter().enumerate() {
                for (b, rb) in centers.iter().skip(i + 1) {
                    assert!(
                        (a - b).magnitude() > ra + rb,
                        "zones at {a:?} and {b:?} overlap with an eye at {eye}"
                    );
                }
            }
        }
    }

    /// A hand in a zone reports it; a hand at the player's own eye - the rest
    /// pose the wrist canvases live at - reports nothing, so reading the watch
    /// can never arm a body-anchor gesture.
    #[test]
    fn anchors_answer_only_inside_their_own_zone() {
        let mut tracker = BodyFrameTracker::default();
        let frame = tracker.update(&standing(Quaternion::from_angle_y(Deg(0.0))));

        assert_eq!(frame.anchor_at(frame.belt()), Some(BodyAnchor::Belt));
        for hand in [Handedness::Left, Handedness::Right] {
            assert_eq!(
                frame.anchor_at(frame.shoulder(hand)),
                Some(BodyAnchor::Shoulder(hand))
            );
        }

        let eye = vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0);
        assert_eq!(frame.anchor_at(eye), None, "the eye is not an anchor");
        assert_eq!(
            frame.anchor_at(eye + vec3(0.0, 0.0, -meters(0.35))),
            None,
            "a hand held up in front of the face is not an anchor"
        );
    }

    fn at(anchor: Option<BodyAnchor>) -> HandAnchorInput {
        HandAnchorInput {
            hand: Handedness::Right,
            squeeze: 0.0,
            anchor,
            holding: false,
            holds_card: false,
            can_stow: true,
            can_draw: true,
            has_card: true,
        }
    }

    const SHOULDER: Option<BodyAnchor> = Some(BodyAnchor::Shoulder(Handedness::Right));

    /// Opening a full hand over a shoulder stows; closing an empty one there
    /// draws.
    #[test]
    fn a_shoulder_stows_what_you_hold_and_draws_what_you_stowed() {
        let mut gestures = AnchorGestures::default();
        let mut holding = HandAnchorInput {
            holding: true,
            squeeze: 1.0,
            ..at(SHOULDER)
        };
        assert_eq!(gestures.update(&holding), None, "still gripping");
        holding.squeeze = 0.0;
        assert_eq!(
            gestures.update(&holding),
            Some(AnchorGesture::Stow(Handedness::Right))
        );

        let mut empty = at(SHOULDER);
        assert_eq!(
            gestures.update(&empty),
            None,
            "an open empty hand does not draw"
        );
        empty.squeeze = 1.0;
        assert_eq!(
            gestures.update(&empty),
            Some(AnchorGesture::Draw(Handedness::Right))
        );
        assert_eq!(gestures.update(&empty), None, "a held grip fires once");
    }

    /// Tracking behind the shoulder is the worst tracking there is. A pose that
    /// drops out for a few frames must not turn the release into a world drop.
    #[test]
    fn a_lost_pose_behind_the_shoulder_still_stows() {
        let mut gestures = AnchorGestures::default();
        let holding = |squeeze: f32, anchor| HandAnchorInput {
            holding: true,
            squeeze,
            ..at(anchor)
        };
        gestures.update(&holding(1.0, SHOULDER));
        for _ in 0..ANCHOR_HOLD_FRAMES {
            assert_eq!(gestures.update(&holding(1.0, None)), None);
        }
        assert_eq!(
            gestures.update(&holding(0.0, None)),
            Some(AnchorGesture::Stow(Handedness::Right)),
            "a release inside the hysteresis window still lands in the backpack"
        );

        // Past the window the hand really has left, and the release is an
        // ordinary world drop again.
        let mut gestures = AnchorGestures::default();
        gestures.update(&holding(1.0, SHOULDER));
        for _ in 0..=ANCHOR_HOLD_FRAMES {
            gestures.update(&holding(1.0, None));
        }
        assert_eq!(gestures.update(&holding(0.0, None)), None);
    }

    /// Nothing stowed, or no room to stow: no gesture, and the light says so
    /// before the player commits.
    #[test]
    fn an_unavailable_shoulder_refuses_and_pre_lights_amber() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let empty = HandAnchorInput {
            can_draw: false,
            ..at(SHOULDER)
        };
        gestures.update(&empty);
        assert_eq!(gestures.affordance(&empty), Some(HandAffordance::Blocked));
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..empty
            }),
            None
        );

        let mut gestures = AnchorGestures::default();
        let full = HandAnchorInput {
            holding: true,
            can_stow: false,
            squeeze: 1.0,
            ..at(SHOULDER)
        };
        gestures.update(&full);
        assert_eq!(gestures.affordance(&full), Some(HandAffordance::Blocked));
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 0.0,
                ..full
            }),
            None,
            "a full backpack leaves the release an ordinary world drop"
        );
    }

    /// The belt yields the card to an empty hand, and the card comes back to
    /// the belt wherever it is released - it is never dropped in the world.
    #[test]
    fn the_belt_lends_the_card_and_takes_it_back() {
        let mut gestures = AnchorGestures::default();
        let mut hand = at(Some(BodyAnchor::Belt));
        gestures.update(&hand);
        hand.squeeze = 1.0;
        assert_eq!(
            gestures.update(&hand),
            Some(AnchorGesture::TakeCard(Handedness::Right))
        );

        let mut carrying = HandAnchorInput {
            holds_card: true,
            squeeze: 1.0,
            anchor: None,
            ..at(None)
        };
        assert_eq!(gestures.update(&carrying), None);
        carrying.squeeze = 0.0;
        assert_eq!(
            gestures.update(&carrying),
            Some(AnchorGesture::ReturnCard(Handedness::Right))
        );
    }

    /// With no credential collected there is no card to take, and the belt
    /// says so rather than silently doing nothing.
    #[test]
    fn an_empty_belt_offers_nothing() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let hand = HandAnchorInput {
            has_card: false,
            ..at(Some(BodyAnchor::Belt))
        };
        gestures.update(&hand);
        assert_eq!(gestures.affordance(&hand), Some(HandAffordance::Blocked));
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..hand
            }),
            None
        );
    }

    /// Away from every anchor the hand is an ordinary hand, and the light is
    /// left to its own raycast.
    #[test]
    fn away_from_the_body_nothing_is_claimed() {
        let mut gestures = AnchorGestures::default();
        let mut hand = HandAnchorInput {
            holding: true,
            squeeze: 1.0,
            ..at(None)
        };
        assert_eq!(gestures.affordance(&hand), None);
        gestures.update(&hand);
        hand.squeeze = 0.0;
        assert_eq!(gestures.update(&hand), None);
    }

    /// The belt is on the LEFT hip, and turns with the body rather than staying
    /// in world space.
    #[test]
    fn the_belt_rides_the_left_hip() {
        let mut tracker = BodyFrameTracker::default();
        let frame = tracker.update(&standing(Quaternion::from_angle_y(Deg(0.0))));
        // Facing -Z, the player's left is -X.
        assert!(frame.belt().x < 0.0, "belt should be on the left hip");
        assert!(frame.shoulder(Handedness::Left).x < 0.0);
        assert!(frame.shoulder(Handedness::Right).x > 0.0);

        // The pawn turned 90 deg with the head straight ahead: facing -X, so
        // the player's left is +Z.
        let mut turned = standing(Quaternion::from_angle_y(Deg(0.0)));
        turned.pawn_rotation = Quaternion::from_angle_y(Deg(90.0));
        let frame = BodyFrameTracker::default().update(&turned);
        assert!(
            frame.belt().z > 0.0,
            "facing -X the left hip is at +Z, got {:?}",
            frame.belt()
        );
    }
}
