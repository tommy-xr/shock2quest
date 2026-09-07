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
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

use crate::{body_frame::BodyFrame, vr_config::Handedness};

/// The gamesys's own ID-card art (`ID Cards`, template -157, `P$ModelName`) -
/// the model every collected card in the game already wears.
const CARD_MODEL: &str = "scipass";

/// How far in front of the card a reader counts as touched, world units
/// (~15 cm). Short on purpose: the card is placed against the panel, not
/// pointed at it from across the room.
pub const READER_REACH: f32 = 0.2;

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
    Matrix4::from_translation(frame.belt())
        * Matrix4::from(frame.rotation())
        * Matrix4::from_angle_x(cgmath::Deg(-90.0))
        * Matrix4::from_scale(crate::vr_config::held_geometry_scale(CARD_MODEL))
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

/// The card's renderable geometry at `transform`, or nothing if the model is
/// missing (a data set without it simply has no belt card).
pub fn scene_objects(asset_cache: &mut AssetCache, transform: Matrix4<f32>) -> Vec<SceneObject> {
    let Some(model) = asset_cache.get_opt::<_, dark::model::Model, _>(
        &dark::importers::MODELS_IMPORTER,
        &format!("{CARD_MODEL}.BIN"),
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
