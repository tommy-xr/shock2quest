//! Renders the SteamVR glove model as the player's VR hands, posed from the
//! controller's analog inputs (trigger curls the index finger, squeeze curls
//! the rest). The left hand mirrors the right-hand model (`flip_x`-style
//! negative scale), like held-weapon models do.

use std::cell::RefCell;
use std::rc::Rc;

use cgmath::{Matrix4, Quaternion, Vector3};
use dark::{
    glb_model::GlbModel,
    importers::{GLB_MODELS_IMPORTER, TEXTURE_IMPORTER},
};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{Material, SceneObject, SkinnedMaterial},
};

use crate::{
    hand_pose::{self, FingerAmounts, HandPoseRetarget, Pose},
    vr_config::Handedness,
};

const GLOVE_MODEL: &str = "vr_glove_model.glb";
const GLOVE_TEXTURE: &str = "vr_glove_color.jpg";

/// Everything constant about the glove, resolved once: the model (a private
/// clone whose skeleton is re-posed each frame), the pose retargeting, the
/// blend endpoints, and one textured material per mesh (fresh cells so the
/// cached asset's materials are never mutated).
pub struct GloveRenderer {
    model: GlbModel,
    retarget: HandPoseRetarget,
    open: Pose,
    fist: Pose,
    materials: Vec<Rc<RefCell<Box<dyn Material>>>>,
}

impl GloveRenderer {
    /// `None` when the glove model is missing or has no skeleton. Callers
    /// should cache that outcome rather than retry every frame (an asset
    /// cache miss is the expensive path).
    pub fn new(asset_cache: &mut AssetCache) -> Option<Self> {
        let model = asset_cache.get_opt(&GLB_MODELS_IMPORTER, GLOVE_MODEL)?;
        if !model.has_skeleton() {
            return None;
        }
        let model = model.as_ref().clone();

        let texture = asset_cache
            .get_opt::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, GLOVE_TEXTURE)
            .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>);

        // One material per mesh; without the external texture, keep the
        // materials the importer built (solid-color fallback).
        let materials = model
            .to_scene_objects()
            .iter()
            .map(|object| match &texture {
                Some(texture) => Rc::new(RefCell::new(SkinnedMaterial::create(
                    texture.clone(),
                    1.0,
                    0.0,
                ))),
                None => object.material.clone(),
            })
            .collect();

        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());

        Some(Self {
            model,
            retarget,
            open: hand_pose::open_right_hand(),
            fist: hand_pose::fist_right_hand(),
            materials,
        })
    }

    /// Build the posed glove scene objects for one hand at its world transform.
    pub fn render_hand(
        &mut self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
        trigger_value: f32,
        squeeze_value: f32,
    ) -> Vec<SceneObject> {
        // Index follows the trigger; the other fingers follow the squeeze. A
        // full squeeze also curls the index so a gripped fist looks like a fist.
        let amounts = FingerAmounts {
            thumb: squeeze_value,
            index: trigger_value.max(squeeze_value),
            middle: squeeze_value,
            ring: squeeze_value,
            pinky: squeeze_value,
        };
        let pose = self.open.blend_per_finger(&self.fist, &amounts);
        self.retarget.apply(&pose, &mut self.model);

        // The right-hand model is mirrored across the hand's local X for the
        // left hand (same trick as vr_config::flip_x for held weapons). The
        // glove's fingers point along the model's +Z; the hand frame's
        // forward is -Z (the raycast/aim direction, see VirtualHand::update),
        // so the grip alignment yaws the model 180 degrees to line the
        // fingers up with where the hand points. Verified against the
        // raycast hit markers in-game; on-headset fine tuning would adjust
        // this rotation.
        let grip = Matrix4::from_angle_y(cgmath::Deg(180.0));
        let mirror = match handedness {
            Handedness::Right => Matrix4::from_scale(1.0),
            Handedness::Left => Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0),
        };
        let world = Matrix4::from_translation(position) * Matrix4::from(rotation) * mirror * grip;

        let mut objects = self.model.to_scene_objects_with_skinning();
        for (object, material) in objects.iter_mut().zip(&self.materials) {
            object.material = material.clone();
            object.set_transform(world);
        }
        objects
    }
}
