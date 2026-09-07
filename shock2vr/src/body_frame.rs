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

use cgmath::{InnerSpace, Matrix4, Quaternion, Rad, Rotation, Rotation3, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

use crate::{util::meters, vr_config::Handedness};

/// Belt height as a fraction of the tracked eye height. ~0.55 puts it at the
/// top of the hip on an average adult, which is where a real belt sits and
/// where the hand naturally falls when the arm hangs and the elbow bends.
const BELT_HEIGHT_FRACTION: f32 = 0.55;

/// How far the shoulder sockets sit below the eye.
const SHOULDER_DROP: f32 = meters(0.2);

/// How far the shoulder sockets sit BEHIND the eye. The head's floor
/// projection is roughly the neck, not the shoulder joint - without this the
/// zone sits beside the ear, and the "throw it over your shoulder" reach (which
/// takes the hand back past the head) never enters it.
const SHOULDER_BEHIND: f32 = meters(0.12);

/// Half the shoulder/hip width: how far each anchor sits to the side of the
/// body's midline.
const LATERAL_OFFSET: f32 = meters(0.18);

/// Reach radius of each anchor. The hips carry small objects the hand has to
/// find; a shoulder is a "throw it back there" gesture and is deliberately more
/// forgiving.
const HIP_RADIUS: f32 = meters(0.09);
const SHOULDER_RADIUS: f32 = meters(0.15);

/// Reach radius of the thigh holster. Between the two: the holster is a
/// deliberate reach for a specific object like a hip, but it hangs off a leg
/// the player cannot see and has no tracked pose for, so it is given more room.
const HOLSTER_RADIUS: f32 = meters(0.12);

/// Minimum vertical gap between the hips and the shoulders, so a deep crouch
/// (which lowers the eye, and with it both anchors) can never bring the two
/// zones into contact and make "which anchor is this" a matter of rounding.
const ANCHOR_SEPARATION: f32 = HIP_RADIUS + SHOULDER_RADIUS + meters(0.05);

/// The same guarantee between the holster and the hip above it. Enforced as a
/// clamp rather than trusted to the height fraction, because that fraction is a
/// live dev param: no setting of it may make "which anchor is this" ambiguous.
const HOLSTER_SEPARATION: f32 = HIP_RADIUS + HOLSTER_RADIUS + meters(0.05);

/// Time constant of the yaw low-pass, in seconds. Long enough that a glance
/// leaves the body where it was, short enough that turning to walk brings the
/// belt round with you well before you reach for it.
const YAW_TIME_CONSTANT_SECS: f32 = 0.5;

/// A place on the player's body a hand can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyAnchor {
    /// Left hip: the belt card.
    Belt,
    /// Right hip: the ammo pouch that hands out clips for the wielded gun.
    Pouch,
    /// Either shoulder: the backpack (stow what you hold, draw what you stowed).
    Shoulder(Handedness),
    /// The dominant thigh: the one holster slot, which holds one weapon.
    Holster,
}

/// Which hip the holster hangs beside this frame.
pub fn holster_side() -> Handedness {
    let dominant = crate::vr_config::dominant_hand();
    if crate::dev_params::get_bool(crate::dev_params::HOLSTER_SIDE_FLIPPED) {
        crate::vr_config::other_hand(dominant)
    } else {
        dominant
    }
}

/// This frame's resolved body anchors, in world space.
#[derive(Clone, Copy, Debug)]
pub struct BodyFrame {
    belt: Vector3<f32>,
    pouch: Vector3<f32>,
    holster: Vector3<f32>,
    /// Indexed by [`crate::vr_config::hand_slot`].
    shoulders: [Vector3<f32>; 2],
    rotation: Quaternion<f32>,
}

impl BodyFrame {
    /// Where the belt card sits.
    pub fn belt(&self) -> Vector3<f32> {
        self.belt
    }

    /// Where the ammo pouch sits.
    pub fn pouch(&self) -> Vector3<f32> {
        self.pouch
    }

    /// Where the weapon holster hangs.
    pub fn holster(&self) -> Vector3<f32> {
        self.holster
    }

    /// Where one shoulder socket sits.
    pub fn shoulder(&self, hand: Handedness) -> Vector3<f32> {
        self.shoulders[crate::vr_config::hand_slot(hand)]
    }

    /// The body's facing - yaw only, so anything parented to it stays upright.
    pub fn rotation(&self) -> Quaternion<f32> {
        self.rotation
    }

    /// Which anchor, if any, a hand at `position` is inside. The hips are
    /// tested first: they are the smaller, more precisely-placed zones, and
    /// [`ANCHOR_SEPARATION`] keeps them from ever overlapping the shoulders
    /// anyway.
    pub fn anchor_at(&self, position: Vector3<f32>) -> Option<BodyAnchor> {
        if (position - self.belt).magnitude2() <= HIP_RADIUS * HIP_RADIUS {
            return Some(BodyAnchor::Belt);
        }
        if (position - self.pouch).magnitude2() <= HIP_RADIUS * HIP_RADIUS {
            return Some(BodyAnchor::Pouch);
        }
        if (position - self.holster).magnitude2() <= HOLSTER_RADIUS * HOLSTER_RADIUS {
            return Some(BodyAnchor::Holster);
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
    /// (`physics::player_center_above_floor`). Only the belt reads it - the
    /// shoulders hang off the eye, where it cancels - so it must be the same
    /// stance the tracked head was rebased with, or the belt moves out from
    /// under the hand without the shoulders moving with it.
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
                    crate::util::wrap_pi(current + crate::util::wrap_pi(observed - current) * alpha)
                }
            });
        }
        let rotation = input.pawn_rotation * Quaternion::from_angle_y(Rad(self.yaw.unwrap_or(0.0)));
        let right = rotation.rotate_vector(vec3(1.0, 0.0, 0.0));
        let back = rotation.rotate_vector(vec3(0.0, 0.0, 1.0));

        // Horizontal placement follows the head (lean out over a railing and
        // your belt goes with you); heights come off the tracked eye. The head
        // is in the same pawn space the hands are, so it converts to world
        // through the same function the hands tested against these anchors do.
        let floor_y = input.pawn_position.y - input.pawn_above_floor;
        let ground = crate::virtual_hand::hand_world_position(
            input.pawn_position,
            input.pawn_rotation,
            vec3(input.head_position.x, 0.0, input.head_position.z),
        );
        let eye_above_floor = input.head_position.y + input.pawn_above_floor;

        let shoulder_y = floor_y + eye_above_floor - SHOULDER_DROP;
        let belt_y =
            (floor_y + eye_above_floor * BELT_HEIGHT_FRACTION).min(shoulder_y - ANCHOR_SEPARATION);
        let at = |y: f32| vec3(ground.x, y, ground.z);

        // The holster hangs off the same body, a clamped distance below the hip
        // on its own side, so no dev-param setting can slide it into the pouch.
        let holster_y = (floor_y
            + eye_above_floor * crate::dev_params::get(crate::dev_params::HOLSTER_HEIGHT_FRACTION))
        .min(belt_y - HOLSTER_SEPARATION);
        let holster_out = match holster_side() {
            Handedness::Right => 1.0,
            Handedness::Left => -1.0,
        } * meters(crate::dev_params::get(crate::dev_params::HOLSTER_LATERAL));
        let holster_forward = meters(crate::dev_params::get(crate::dev_params::HOLSTER_FORWARD));

        let shoulder = at(shoulder_y) + back * SHOULDER_BEHIND;
        BodyFrame {
            // The card rides the left hip, the ammo pouch the right.
            belt: at(belt_y) - right * LATERAL_OFFSET,
            pouch: at(belt_y) + right * LATERAL_OFFSET,
            holster: at(holster_y) + right * holster_out - back * holster_forward,
            shoulders: [
                shoulder - right * LATERAL_OFFSET,
                shoulder + right * LATERAL_OFFSET,
            ],
            rotation,
        }
    }
}

/// The renderable geometry of a model worn at a body anchor - the belt card,
/// the pouch's clip - placed at `transform`. Nothing if the model is missing,
/// so a data set without it simply wears nothing.
pub fn worn_scene_objects(
    asset_cache: &mut AssetCache,
    model: &str,
    transform: Matrix4<f32>,
) -> Vec<SceneObject> {
    let Some(model) = asset_cache.get_opt::<_, dark::model::Model, _>(
        &dark::importers::MODELS_IMPORTER,
        &format!("{model}.BIN"),
    ) else {
        return Vec::new();
    };
    model
        .clone_scene_objects()
        .into_iter()
        .map(|mut object| {
            object.set_transform(transform);
            object
        })
        .collect()
}

/// How long a hand keeps the anchor it was over after losing it, in fixed
/// 60 Hz frames (~200 ms). A hand reaching behind the shoulder is exactly where
/// tracking is worst, and a pose that drops out for a frame must not read as
/// "left the zone" - the release it is about to make would then land on the
/// floor instead of in the backpack.
pub(crate) const ANCHOR_HOLD_FRAMES: u8 = 12;

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
    /// Gripped at the pouch with an empty hand: take out a clip for the gun the
    /// OTHER hand wields.
    TakeClip(Handedness),
    /// Opened over the pouch holding a clip: its rounds go back to the reserve.
    ReturnClip(Handedness),
    /// Opened at the holster holding a weapon: it goes into the slot.
    Holster(Handedness),
    /// Gripped at an occupied holster with an empty hand: the weapon comes out
    /// into THAT hand.
    Unholster(Handedness),
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
    /// Whether the hand holds an ammo clip - the one thing the pouch takes back.
    pub holds_clip: bool,
    /// Whether the pouch has a gun to serve this hand: the OTHER hand wields
    /// one. Without it the pouch is not drawn and claims nothing, so a hand at
    /// an empty hip goes on interacting with the world.
    pub pouch_serves: bool,
    /// Whether that gun's selected ammo has a compatible clip left in reserve.
    pub pouch_clip_available: bool,
    /// Whether the backpack could take what the hand holds.
    pub can_stow: bool,
    /// Whether there is a stowed weapon left to draw.
    pub can_draw: bool,
    /// Whether what the hand holds is a weapon the holster takes - a gun or a
    /// melee weapon, but not the psi amp and not an ordinary pickup.
    pub holds_weapon: bool,
    /// Whether the holster already has a weapon in it. One slot: an occupied
    /// holster refuses a second weapon, and an empty one has nothing to draw.
    pub holster_occupied: bool,
    /// Whether the card is actually on the belt to be taken: the player has
    /// collected a credential AND no hand is already carrying it. There is one
    /// card, so a hand cannot take it out of the other hand's grip.
    pub card_on_belt: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct HandAnchorState {
    squeezing: bool,
    /// The anchor this hand is treated as being at - the live one, or the last
    /// one for [`ANCHOR_HOLD_FRAMES`] after the pose is lost. One value, so the
    /// gesture a grip commits to and the claim [`AnchorGestures::claim`] makes
    /// on the hand can never disagree about where the hand is.
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
    /// Advance one hand a frame, and return the gesture its grip edge committed
    /// to. [`Self::claim`] must be read after this - both answers come from the
    /// one intent this resolves.
    pub fn update(&mut self, input: &HandAnchorInput) -> Option<AnchorGesture> {
        let state = &mut self.hands[crate::vr_config::hand_slot(input.hand)];
        match input.anchor {
            Some(anchor) => {
                state.anchor = Some(anchor);
                state.hold_frames = ANCHOR_HOLD_FRAMES;
            }
            // The window is spent frame by frame; only when it runs out is the
            // anchor really gone.
            None if state.hold_frames > 0 => state.hold_frames -= 1,
            None => state.anchor = None,
        }

        // Thresholded to match `VirtualHand`'s own hold test exactly: a squeeze
        // sitting on the threshold must not read as released here while the
        // hand still holds the item, or a stow would claim a release the hand
        // never made and the item would land on the floor a frame later.
        let squeezing = input.squeeze >= crate::ui::VR_TRIGGER_THRESHOLD;
        let was_squeezing = std::mem::replace(&mut state.squeezing, squeezing);

        // Only a real grip edge commits. Losing the pose behind the shoulder
        // changes nothing: the controller keeps reporting its grip.
        let (gesture, available) = self.intent(input)?;
        let fires = match gesture {
            // Letting go is what puts something away.
            AnchorGesture::Stow(_)
            | AnchorGesture::ReturnCard(_)
            | AnchorGesture::ReturnClip(_)
            | AnchorGesture::Holster(_) => was_squeezing && !squeezing,
            // Closing on it is what takes something out.
            AnchorGesture::Draw(_)
            | AnchorGesture::TakeCard(_)
            | AnchorGesture::TakeClip(_)
            | AnchorGesture::Unholster(_) => !was_squeezing && squeezing,
        };
        (fires && available).then_some(gesture)
    }

    /// What owns this hand this frame, if anything. Must be read *after*
    /// [`Self::update`], which resolves the anchor both answers come from.
    pub fn claim(&self, input: &HandAnchorInput) -> Option<HandClaim> {
        use crate::hand_affordance::HandAffordance;

        let (gesture, available) = self.intent(input)?;
        Some(match gesture {
            AnchorGesture::ReturnCard(_) => HandClaim::CarryingCard,
            _ => HandClaim::Anchor(if available {
                HandAffordance::Grabbable
            } else {
                HandAffordance::Blocked
            }),
        })
    }

    /// What this hand's next grip edge would do, and whether it could. The one
    /// place the rules live: the light and the committed gesture both read it,
    /// so the glove can never promise an action the grip would not perform.
    fn intent(&self, input: &HandAnchorInput) -> Option<(AnchorGesture, bool)> {
        // A hand carrying the belt card is full, wherever it is: it must not
        // also grab a pickup or frob what its ray crosses, because the card's
        // own touch already frobs (and a second Frob would shut the door the
        // first opened). Opening it returns the card to the belt.
        if input.holds_card {
            return Some((AnchorGesture::ReturnCard(input.hand), true));
        }
        Some(
            match self.hands[crate::vr_config::hand_slot(input.hand)].anchor? {
                BodyAnchor::Shoulder(_) if input.holding => {
                    (AnchorGesture::Stow(input.hand), input.can_stow)
                }
                BodyAnchor::Shoulder(_) => (AnchorGesture::Draw(input.hand), input.can_draw),
                BodyAnchor::Belt if !input.holding => {
                    (AnchorGesture::TakeCard(input.hand), input.card_on_belt)
                }
                // A full hand at the belt has nowhere to put what it holds.
                BodyAnchor::Belt => return None,
                // The pouch takes clips back and hands them out. Anything else
                // in the hand is recognised and refused rather than silently
                // ignored - the light says the pouch will not take it.
                BodyAnchor::Pouch if input.holding => (
                    AnchorGesture::ReturnClip(input.hand),
                    input.holds_clip && input.can_stow,
                ),
                // No gun wielded means no pouch on the hip at all, so there is
                // nothing to light up or grip.
                BodyAnchor::Pouch if !input.pouch_serves => return None,
                BodyAnchor::Pouch => (
                    AnchorGesture::TakeClip(input.hand),
                    input.pouch_clip_available,
                ),
                // One slot, weapons only. A full hand is always answered - with
                // amber when the slot is taken or the thing held is not a
                // weapon - so the light says the holster will not have it
                // rather than leaving the hand to guess.
                BodyAnchor::Holster if input.holding => (
                    AnchorGesture::Holster(input.hand),
                    input.holds_weapon && !input.holster_occupied && input.can_stow,
                ),
                BodyAnchor::Holster => {
                    (AnchorGesture::Unholster(input.hand), input.holster_occupied)
                }
            },
        )
    }
}

/// Why a hand is not an ordinary world-interacting hand this frame. Either way
/// its grip belongs to the body, not to whatever its ray crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandClaim {
    /// The hand is at a body anchor. The value is the anchor's eligibility -
    /// green when the gesture would go through, amber when the anchor is
    /// recognised but the action is not available (a full backpack, nothing
    /// stowed to draw, an empty belt).
    Anchor(crate::hand_affordance::HandAffordance),
    /// The hand is carrying the belt card. Its light keeps reading its own
    /// raycast, so a reader it is held against still shows locked or openable.
    CarryingCard,
}

/// The yaw a tracked head faces, radians about +Y, or `None` for an untracked
/// (zero-magnitude) pose - which is not a reading, and must not be smoothed
/// toward. A head looking straight down at its own belt is exactly the pose
/// whose horizontal forward vanishes, so the flattening is
/// [`crate::util::horizontal_forward`]'s rather than a bare `atan2` on `xz`.
fn head_yaw(rotation: Quaternion<f32>) -> Option<f32> {
    let forward = crate::util::horizontal_forward(crate::util::tracked_rotation(rotation)?);
    // `Quaternion::from_angle_y(t) * (0, 0, -1)` is `(-sin t, 0, -cos t)`.
    Some((-forward.x).atan2(-forward.z))
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

    /// The four zones never overlap, standing or crouched - a hand is only
    /// ever in one of them, so the gesture it commits to is never a matter of
    /// test order.
    #[test]
    fn the_anchor_zones_stay_disjoint_at_every_stance() {
        for eye in [0.5, 1.04, 1.4] {
            let mut input = standing(Quaternion::from_angle_y(Deg(0.0)));
            input.head_position.y = eye;
            let frame = BodyFrameTracker::default().update(&input);

            let centers = [
                (frame.belt(), HIP_RADIUS),
                (frame.pouch(), HIP_RADIUS),
                (frame.holster(), HOLSTER_RADIUS),
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
        assert_eq!(frame.anchor_at(frame.pouch()), Some(BodyAnchor::Pouch));
        assert_eq!(frame.anchor_at(frame.holster()), Some(BodyAnchor::Holster));
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
            holds_clip: false,
            pouch_serves: true,
            pouch_clip_available: true,
            can_stow: true,
            can_draw: true,
            holds_weapon: true,
            holster_occupied: false,
            card_on_belt: true,
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
        // The pose stays lost for the whole window; the release lands on its
        // last frame and still stows.
        for _ in 1..ANCHOR_HOLD_FRAMES {
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
        assert_eq!(
            gestures.claim(&empty),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
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
        assert_eq!(
            gestures.claim(&full),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
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
            card_on_belt: false,
            ..at(Some(BodyAnchor::Belt))
        };
        gestures.update(&hand);
        assert_eq!(
            gestures.claim(&hand),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..hand
            }),
            None
        );
    }

    /// A hand carrying the card is claimed wherever it is - it must not also
    /// grab a pickup its ray crosses on the way to a reader, and the shoulder
    /// must not promise a stow the release would not perform.
    #[test]
    fn a_hand_carrying_the_card_is_claimed_everywhere() {
        let mut gestures = AnchorGestures::default();
        for anchor in [None, SHOULDER, Some(BodyAnchor::Belt)] {
            let hand = HandAnchorInput {
                holds_card: true,
                squeeze: 1.0,
                ..at(anchor)
            };
            gestures.update(&hand);
            assert_eq!(
                gestures.claim(&hand),
                Some(HandClaim::CarryingCard),
                "the card fills the hand at {anchor:?}"
            );
            assert_eq!(
                gestures.update(&HandAnchorInput {
                    squeeze: 0.0,
                    ..hand
                }),
                Some(AnchorGesture::ReturnCard(Handedness::Right)),
                "and opening it returns the card rather than stowing"
            );
        }
    }

    /// There is one card. A hand at the belt while the other carries it finds
    /// the hip empty rather than taking it out of that grip.
    #[test]
    fn the_belt_is_empty_while_the_other_hand_has_the_card() {
        let mut gestures = AnchorGestures::default();
        let hand = HandAnchorInput {
            hand: Handedness::Left,
            card_on_belt: false,
            ..at(Some(BodyAnchor::Belt))
        };
        gestures.update(&hand);
        assert_eq!(
            gestures.claim(&hand),
            Some(HandClaim::Anchor(
                crate::hand_affordance::HandAffordance::Blocked
            ))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..hand
            }),
            None,
            "the second hand must not take the card out of the first"
        );
    }

    /// The light and the gesture read one resolved anchor: a hand the
    /// hysteresis has let go of neither commits nor claims.
    #[test]
    fn an_expired_anchor_neither_claims_nor_commits() {
        let mut gestures = AnchorGestures::default();
        let empty = |anchor| at(anchor);
        gestures.update(&empty(SHOULDER));
        for _ in 0..=ANCHOR_HOLD_FRAMES {
            gestures.update(&empty(None));
        }
        assert_eq!(gestures.claim(&empty(None)), None);
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..empty(None)
            }),
            None,
            "an expired anchor must not draw into a hand it no longer owns"
        );
    }

    /// The release edge is the same one `VirtualHand` drops on. A squeeze
    /// resting exactly on the threshold is still a hold, so a stow cannot claim
    /// a release the hand never made.
    #[test]
    fn the_release_edge_matches_the_hands_own_hold_test() {
        let mut gestures = AnchorGestures::default();
        let holding = |squeeze: f32| HandAnchorInput {
            holding: true,
            squeeze,
            ..at(SHOULDER)
        };
        gestures.update(&holding(1.0));
        assert_eq!(
            gestures.update(&holding(crate::ui::VR_TRIGGER_THRESHOLD)),
            None,
            "a squeeze on the threshold still holds, so nothing is stowed"
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
        assert_eq!(gestures.claim(&hand), None);
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
        assert!(frame.pouch().x > 0.0, "pouch should be on the right hip");
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
        assert!(
            frame.pouch().z < 0.0,
            "facing -X the right hip is at -Z, got {:?}",
            frame.pouch()
        );
    }

    const POUCH: Option<BodyAnchor> = Some(BodyAnchor::Pouch);

    /// The pouch lends a clip to an empty hand and takes one back when the hand
    /// opens over it.
    #[test]
    fn the_pouch_lends_a_clip_and_takes_one_back() {
        let mut gestures = AnchorGestures::default();
        let mut hand = at(POUCH);
        gestures.update(&hand);
        hand.squeeze = 1.0;
        assert_eq!(
            gestures.update(&hand),
            Some(AnchorGesture::TakeClip(Handedness::Right))
        );

        let mut carrying = HandAnchorInput {
            holding: true,
            holds_clip: true,
            squeeze: 1.0,
            ..at(POUCH)
        };
        assert_eq!(gestures.update(&carrying), None);
        carrying.squeeze = 0.0;
        assert_eq!(
            gestures.update(&carrying),
            Some(AnchorGesture::ReturnClip(Handedness::Right))
        );
    }

    /// A gun with nothing compatible left in reserve pre-lights amber and
    /// refuses the grip, rather than handing out rounds that do not exist.
    #[test]
    fn a_pouch_with_no_compatible_clip_refuses_and_pre_lights_amber() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let empty = HandAnchorInput {
            pouch_clip_available: false,
            ..at(POUCH)
        };
        gestures.update(&empty);
        assert_eq!(
            gestures.claim(&empty),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..empty
            }),
            None
        );
    }

    /// With no gun wielded there is no pouch on the hip, so the hand is left
    /// to the world rather than claimed by an anchor that draws nothing.
    #[test]
    fn an_unserved_pouch_claims_nothing() {
        let mut gestures = AnchorGestures::default();
        let hand = HandAnchorInput {
            pouch_serves: false,
            ..at(POUCH)
        };
        gestures.update(&hand);
        assert_eq!(gestures.claim(&hand), None);
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..hand
            }),
            None
        );
    }

    /// The pouch holds clips, not everything: a hand full of something else
    /// says so and leaves the release an ordinary world drop.
    #[test]
    fn the_pouch_refuses_what_is_not_a_clip() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let full = HandAnchorInput {
            holding: true,
            holds_clip: false,
            squeeze: 1.0,
            ..at(POUCH)
        };
        gestures.update(&full);
        assert_eq!(
            gestures.claim(&full),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 0.0,
                ..full
            }),
            None
        );
    }

    const HOLSTER: Option<BodyAnchor> = Some(BodyAnchor::Holster);

    /// The holster hangs below the belt on the dominant side, and a head that
    /// tilts or rolls does not take it with it - the frame is yaw-only, so a
    /// player looking down at their own thigh finds the holster where they left
    /// it rather than swung round with their gaze.
    #[test]
    fn the_holster_rides_the_dominant_thigh_and_ignores_head_tilt() {
        let mut tracker = BodyFrameTracker::default();
        let level = tracker.update(&standing(Quaternion::from_angle_y(Deg(0.0))));

        let right = level.rotation().rotate_vector(vec3(1.0, 0.0, 0.0));
        assert!(
            (level.holster() - level.pouch()).dot(right) > 0.0,
            "the holster should sit outboard of the pouch on the same (right) side"
        );
        assert!(
            level.holster().y < level.belt().y,
            "the holster {} should hang below the belt {}",
            level.holster().y,
            level.belt().y
        );

        // Same yaw, head pitched down at the thigh and rolled: pitch and roll
        // are not readings the frame takes.
        let mut tilted = standing(
            Quaternion::from_angle_y(Deg(0.0))
                * Quaternion::from_angle_x(Deg(-70.0))
                * Quaternion::from_angle_z(Deg(25.0)),
        );
        tilted.dt = STEP;
        let looked_down = tracker.update(&tilted);
        assert!(
            (looked_down.holster() - level.holster()).magnitude() < 1e-4,
            "a tilted head moved the holster from {:?} to {:?}",
            level.holster(),
            looked_down.holster()
        );
    }

    /// Opening a hand full of weapon at the holster docks it; closing an empty
    /// one there draws it back.
    #[test]
    fn the_holster_takes_a_weapon_and_gives_it_back() {
        let mut gestures = AnchorGestures::default();
        let mut holding = HandAnchorInput {
            holding: true,
            squeeze: 1.0,
            ..at(HOLSTER)
        };
        assert_eq!(gestures.update(&holding), None, "still gripping");
        holding.squeeze = 0.0;
        assert_eq!(
            gestures.update(&holding),
            Some(AnchorGesture::Holster(Handedness::Right))
        );

        let mut empty = HandAnchorInput {
            holster_occupied: true,
            ..at(HOLSTER)
        };
        assert_eq!(gestures.update(&empty), None, "an open hand does not draw");
        empty.squeeze = 1.0;
        assert_eq!(
            gestures.update(&empty),
            Some(AnchorGesture::Unholster(Handedness::Right))
        );
    }

    /// One slot. A second weapon is recognised and refused - amber, not a
    /// silent nothing - and an empty slot has nothing to draw.
    #[test]
    fn an_occupied_holster_refuses_a_second_weapon() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let full = HandAnchorInput {
            holding: true,
            holster_occupied: true,
            squeeze: 1.0,
            ..at(HOLSTER)
        };
        gestures.update(&full);
        assert_eq!(
            gestures.claim(&full),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 0.0,
                ..full
            }),
            None,
            "an occupied holster must not take a second weapon"
        );

        let mut gestures = AnchorGestures::default();
        let empty = at(HOLSTER);
        gestures.update(&empty);
        assert_eq!(
            gestures.claim(&empty),
            Some(HandClaim::Anchor(HandAffordance::Blocked)),
            "nothing holstered means nothing to draw"
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 1.0,
                ..empty
            }),
            None
        );
    }

    /// Weapons only: a hand full of anything else says so at the holster and
    /// keeps its ordinary release.
    #[test]
    fn the_holster_refuses_what_is_not_a_weapon() {
        use crate::hand_affordance::HandAffordance;

        let mut gestures = AnchorGestures::default();
        let full = HandAnchorInput {
            holding: true,
            holds_weapon: false,
            squeeze: 1.0,
            ..at(HOLSTER)
        };
        gestures.update(&full);
        assert_eq!(
            gestures.claim(&full),
            Some(HandClaim::Anchor(HandAffordance::Blocked))
        );
        assert_eq!(
            gestures.update(&HandAnchorInput {
                squeeze: 0.0,
                ..full
            }),
            None
        );
    }
}
