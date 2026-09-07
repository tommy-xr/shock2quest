//! The belt card: one persistent object on the left hip standing in for every
//! credential the player has collected.
//!
//! Keycards are *collected*, not inventoried - frobbing one records it in
//! `QuestInfo` and destroys the pickup, and doors consult the collected set.
//! That leaves the player carrying an invisible key ring, which is fine on a
//! flat screen where a locked door just opens, and unsatisfying in VR where
//! there is a hand to put a card in. So the card here is **presentation**: it
//! appears once any credential is held, never occupies an inventory slot, and
//! is derived from `QuestInfo` every frame rather than saved. Held against a
//! reader it runs the same credential check a frob does - the door's own
//! script owns the refusal - so what it opens is exactly what the collected set
//! opens.

use cgmath::{Matrix4, Quaternion, Rotation3, Vector3};

use crate::{body_frame::BodyFrame, vr_config::Handedness};

/// The gamesys's own ID-card art (`ID Cards`, template -157, `P$ModelName`) -
/// the model every collected card in the game already wears.
pub const CARD_MODEL: &str = "scipass";

/// How far in front of the card a reader counts as touched, world units
/// (~15 cm). Short on purpose: the card is placed against the panel, not
/// pointed at it from across the room.
pub const READER_REACH: f32 = 0.2;

/// How long a touched reader stays claimed after the ray comes off it, in fixed
/// 60 Hz frames (~0.4 s). A hand resting a card on a panel jitters across the
/// collider edge, and re-arming on the first missed frame would send a second
/// `Frob` - which on a door shuts what the first opened.
pub const READER_RELEASE_FRAMES: u8 = 24;

/// Whether an entity reads cards: it authors a key destination
/// (`P$KeyDst`), which is what [`crate::scripts::script_util::is_entity_locked`]
/// consults and what a card slot or a card-locked door carries.
///
/// The gate matters. Without it the card would be a 15 cm use-button that
/// activates any switch, lever or item it brushes past - the hand's own trigger
/// is what does that.
pub fn is_reader(world: &shipyard::World, entity_id: shipyard::EntityId) -> bool {
    use shipyard::Get;
    world
        .borrow::<shipyard::View<dark::properties::PropKeyDst>>()
        .is_ok_and(|key_dst| key_dst.get(entity_id).is_ok())
}

/// Where the card actually is in a hand, and which way it is presented. The
/// reach is measured from the card itself, not from the wrist that carries it.
pub fn reader_ray(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    hand: Handedness,
) -> (Vector3<f32>, Vector3<f32>) {
    use cgmath::{Rotation, Transform};
    let seated = hand_transform(position, rotation, hand);
    let origin = seated.transform_point(cgmath::point3(0.0, 0.0, 0.0));
    // The hand's own aim, the same direction its raycast uses - the card is
    // presented where the hand points.
    (
        cgmath::vec3(origin.x, origin.y, origin.z),
        rotation.rotate_vector(cgmath::vec3(0.0, 0.0, -1.0)),
    )
}

/// Where the belt card is this frame. `None` at the call sites means the
/// player has collected no credential yet, and there is no card to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardPlacement {
    /// On the left hip, following the body frame.
    Belt,
    /// Taken off the belt into a hand.
    InHand(Handedness),
}

/// The card resting on the belt: clipped edge-on to the left hip, standing up
/// and facing the way the body does.
///
/// The model is authored lying flat (its long axis is +Z, its face normal +Y),
/// so standing it up is a quarter turn about X. Its held size comes from the
/// same `vr_grips` profile the hand uses, so belt and hand can never disagree
/// about how big the card is.
pub fn belt_transform(frame: &BodyFrame) -> Matrix4<f32> {
    crate::body_frame::worn_transform(
        frame,
        frame.belt(),
        crate::vr_config::held_geometry_scale(CARD_MODEL),
        Quaternion::from_angle_x(cgmath::Deg(-90.0)),
    )
}

/// The card in a hand, seated the way any held pickup is - so it sits in the
/// glove's fingers rather than floating at the wrist.
pub fn hand_transform(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    hand: Handedness,
) -> Matrix4<f32> {
    crate::hand_glove::hand_to_world(position, rotation, hand)
        * crate::vr_config::held_model_hand_transform(CARD_MODEL, hand, 1.0)
}
