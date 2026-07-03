//! Renders the SteamVR glove model as the player's VR hands, posed from the
//! controller's analog inputs (trigger curls the index finger, squeeze curls
//! the rest). The left hand mirrors the right-hand model (`flip_x`-style
//! negative scale), like held-weapon models do.

use std::rc::Rc;

use cgmath::{Deg, Matrix4, Quaternion, Vector3};
use dark::importers::{GLB_MODELS_IMPORTER, TEXTURE_IMPORTER};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, scene::SkinnedMaterial};

use crate::{
    hand_pose::{self, FingerAmounts, HandPoseRetarget},
    vr_config::Handedness,
};

const GLOVE_MODEL: &str = "vr_glove_model.glb";
const GLOVE_TEXTURE: &str = "vr_glove_color.jpg";

/// Rotation from the glove's model space onto the controller grip frame.
/// The glove already matches the hand frame: fingers along +Z (the direction
/// weapon barrels face - see vr_config's rotate_y(-90) for SS2 models), palm
/// inward, thumb up (handshake orientation). Kept as an identity hook for
/// on-headset fine-tuning.
fn grip_rotation() -> Matrix4<f32> {
    Matrix4::from_angle_y(Deg(0.0))
}

/// Build the posed glove scene objects for one hand at its world transform.
pub fn render_glove_hand(
    asset_cache: &mut AssetCache,
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    handedness: Handedness,
    trigger_value: f32,
    squeeze_value: f32,
) -> Vec<SceneObject> {
    let model = asset_cache.get(&GLB_MODELS_IMPORTER, GLOVE_MODEL);
    if !model.has_skeleton() {
        return Vec::new();
    }

    // Index follows the trigger; the other fingers follow the squeeze. A full
    // squeeze also curls the index so a gripped fist looks like a fist.
    let amounts = FingerAmounts {
        thumb: squeeze_value,
        index: trigger_value.max(squeeze_value),
        middle: squeeze_value,
        ring: squeeze_value,
        pinky: squeeze_value,
    };
    let pose =
        hand_pose::open_right_hand().blend_per_finger(&hand_pose::fist_right_hand(), &amounts);

    let mut posed_model = model.as_ref().clone();
    let retarget = HandPoseRetarget::for_right_glove(posed_model.skeleton());
    retarget.apply(&pose, &mut posed_model);

    // The right-hand model is mirrored across the hand's local X for the left
    // hand (same trick as vr_config::flip_x for held weapons).
    let mirror = match handedness {
        Handedness::Right => Matrix4::from_scale(1.0),
        Handedness::Left => Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0),
    };
    let world =
        Matrix4::from_translation(position) * Matrix4::from(rotation) * mirror * grip_rotation();

    let texture = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        asset_cache.get::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, GLOVE_TEXTURE)
    }))
    .ok()
    .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>);

    let mut objects = posed_model.to_scene_objects_with_skinning();
    for object in objects.iter_mut() {
        if let Some(texture) = &texture {
            *object.material.borrow_mut() = SkinnedMaterial::create(texture.clone(), 1.0, 0.0);
        }
        object.set_transform(world);
    }
    objects
}
