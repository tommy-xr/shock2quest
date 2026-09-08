//! Renders the SteamVR glove model as the player's VR hands, posed from the
//! controller's analog inputs (trigger curls the index finger, squeeze curls
//! the rest). The left hand mirrors the right-hand model (`flip_x`-style
//! negative scale), like held-weapon models do.
//!
//! The glove keeps its authored colour map and ends at its authored wrist cuff.

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

/// Authored glove colour map, shared with the pose inspection scene.
const GLOVE_TEXTURE: &str = "vr_glove_color.jpg";

/// Wrist-to-fingertip length the glove model is authored at, in world units -
/// the +Z span of its bind-pose bounding box (fingers point along +Z).
pub const AUTHORED_HAND_LENGTH_WORLD: f32 = 0.2049;

/// Wrist-to-fingertip length of an adult hand. Anthropometric mean is ~19 cm.
const REAL_HAND_LENGTH_METERS: f32 = 0.19;

/// Corrects the glove to life size.
///
/// VR renders the world at true scale - tracked poses are divided by
/// [`crate::METERS_PER_WORLD_UNIT`], so a world unit really is 0.762 m - which
/// means the hand has to be a real hand's size in world units or it reads as
/// too small against everything around it. The model is authored at ~15.6 cm
/// where a hand is ~19 cm, so it renders about a fifth undersized.
///
/// Measured, not guessed: rendering the glove beside a cube of known edge
/// length in `debug_hands` put its bind pose at its authored bounding box, so
/// the shortfall is in the asset, not in the skinning path.
///
/// This is the one number to adjust if life size turns out to read wrong in an
/// actual headset - VR hands are often tuned slightly large.
pub const GLOVE_SCALE: f32 =
    (REAL_HAND_LENGTH_METERS / crate::METERS_PER_WORLD_UNIT) / AUTHORED_HAND_LENGTH_WORLD;

/// Everything constant about the glove, resolved once: the model (a private
/// clone whose skeleton is re-posed each frame), the pose retargeting, the
/// blend endpoints, and one textured material per mesh (fresh cells so the
/// cached asset's materials are never mutated).
pub struct GloveRenderer {
    model: GlbModel,
    retarget: HandPoseRetarget,
    open: Pose,
    fist: Pose,
    point: Pose,
    materials: Vec<Rc<RefCell<Box<dyn Material>>>>,
}

/// An authored pose a hand can be shown in when nothing analog is driving it -
/// the VR frontend pointer, where there is no world to grab and the trigger is
/// just a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticHandPose {
    /// Relaxed, open hand.
    Relaxed,
    /// Index extended, the rest curled.
    Pointing,
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

        let texture = load_glove_texture(asset_cache);

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
            point: hand_pose::point_right_hand(),
            materials,
        })
    }

    /// Sample the renderer's actual retargeted finger arcs once for fitting.
    pub fn grip_kinematics(&mut self, handedness: Handedness) -> crate::vr_grip::GripKinematics {
        use crate::vr_grip::{CURL_STEPS, GripKinematics, glove_to_hand};
        let transform = glove_to_hand(handedness);
        let mut fingers: [Vec<Vec<Vector3<f32>>>; 5] = std::array::from_fn(|_| Vec::new());
        let mut open_joints = Vec::new();
        let mut closed_joints = Vec::new();
        for step in 0..=CURL_STEPS {
            let pose = self.open.blend(&self.fist, step as f32 / CURL_STEPS as f32);
            self.retarget.apply(&pose, &mut self.model);
            let joints = (0..26)
                .map(|joint| {
                    let node = self.model.skeleton().node_index_for_joint(joint).unwrap();
                    (transform * self.model.get_global_transform(node).unwrap())
                        .w
                        .truncate()
                })
                .collect::<Vec<_>>();
            if step == 0 {
                open_joints = joints.clone();
            }
            if step == CURL_STEPS {
                closed_joints = joints.clone();
            }
            for (finger, range) in [3..5, 7..10, 12..15, 17..20, 22..25]
                .into_iter()
                .enumerate()
            {
                let mut points = Vec::new();
                for joint in range {
                    for t in [0.0, 0.5, 1.0] {
                        points.push(joints[joint] * (1.0 - t) + joints[joint + 1] * t);
                    }
                }
                fingers[finger].push(points);
            }
        }
        use cgmath::InnerSpace;
        let knuckles =
            (open_joints[7] + open_joints[12] + open_joints[17] + open_joints[22]) * 0.25;
        let palm = knuckles * 0.8 + open_joints[1] * 0.2;
        let along = (knuckles - open_joints[1]).normalize();
        let across = (open_joints[7] - open_joints[22]).normalize();
        let mut normal = along.cross(across).normalize();
        if normal.dot(closed_joints[15] - palm) < 0.0 {
            normal = -normal;
        }
        GripKinematics {
            fingers,
            palm,
            normal,
        }
    }

    /// Build the posed glove scene objects for one hand at its world transform.
    pub fn render_hand(
        &mut self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
        trigger_value: f32,
        squeeze_value: f32,
        holding: bool,
        fitted: Option<(FingerAmounts, f32)>,
    ) -> Vec<SceneObject> {
        let mut amounts = if holding {
            // Gripping a held item: fingers wrapped on the handle, thumb
            // locked, index resting on the trigger and curling with the pull
            // (the squeeze is what holds the item, so it doesn't drive the
            // pose here). Constants tuned visually against the held pistol.
            FingerAmounts {
                thumb: 0.85,
                index: 0.5 + 0.5 * trigger_value,
                middle: 0.9,
                ring: 0.9,
                pinky: 0.9,
            }
        } else {
            // Empty hand: index follows the trigger; the other fingers follow
            // the squeeze. A full squeeze also curls the index so a squeezed
            // fist looks like a fist.
            FingerAmounts {
                thumb: squeeze_value,
                index: trigger_value.max(squeeze_value),
                middle: squeeze_value,
                ring: squeeze_value,
                pinky: squeeze_value,
            }
        };
        if let Some((fitted, blend)) = fitted {
            let t = blend.clamp(0.0, 1.0);
            amounts = FingerAmounts {
                thumb: amounts.thumb * (1.0 - t) + fitted.thumb * t,
                index: amounts.index * (1.0 - t) + fitted.index * t,
                middle: amounts.middle * (1.0 - t) + fitted.middle * t,
                ring: amounts.ring * (1.0 - t) + fitted.ring * t,
                pinky: amounts.pinky * (1.0 - t) + fitted.pinky * t,
            };
        }
        let pose = self.open.blend_per_finger(&self.fist, &amounts);
        Self::render_posed(
            &mut self.model,
            &self.retarget,
            &self.materials,
            &pose,
            position,
            rotation,
            handedness,
        )
    }

    /// Build the glove in one of the authored [`StaticHandPose`]s.
    ///
    /// The frontend pointer needs this: on a menu there is nothing to grab and
    /// the trigger is a click, so the analog blend `render_hand` does has
    /// nothing to say - the hand should read as "pointing at the panel" or
    /// "not", which is exactly what the authored poses are for.
    pub fn render_static_hand(
        &mut self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
        pose: StaticHandPose,
    ) -> Vec<SceneObject> {
        // Borrowing the pose straight out of the field (rather than cloning
        // its two bone vectors every frame) is why this takes the fields
        // apart instead of `&mut self`.
        let pose = match pose {
            StaticHandPose::Relaxed => &self.open,
            StaticHandPose::Pointing => &self.point,
        };
        Self::render_posed(
            &mut self.model,
            &self.retarget,
            &self.materials,
            pose,
            position,
            rotation,
            handedness,
        )
    }

    /// Bake `pose` into the shared model and place it at the hand's transform.
    /// The one place the model-to-hand frame conversion lives, so every caller
    /// gets the same hand at the same place.
    fn render_posed(
        model: &mut GlbModel,
        retarget: &HandPoseRetarget,
        materials: &[Rc<RefCell<Box<dyn Material>>>],
        pose: &Pose,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
    ) -> Vec<SceneObject> {
        retarget.apply(pose, model);

        // The right-hand model is mirrored across the hand's local X for the
        // left hand - `Handedness::mirror`, the one definition of "the other
        // hand", shared with the melee wield so the glove and the arm rig
        // cannot disagree about which way round the left hand is. The
        // glove's fingers point along the model's +Z; the hand frame's
        // forward is -Z (the raycast/aim direction, see VirtualHand::update),
        // so the grip alignment yaws the model 180 degrees to line the
        // fingers up with where the hand points. Verified against the
        // raycast hit markers in-game; on-headset fine tuning would adjust
        // this rotation.
        let world = Matrix4::from_translation(position)
            * Matrix4::from(rotation)
            * crate::vr_grip::glove_to_hand(handedness);

        let mut objects = model.to_scene_objects_with_skinning();
        for (object, material) in objects.iter_mut().zip(materials) {
            object.material = material.clone();
            object.set_transform(world);
        }

        objects
    }
}

/// The glove's colour map. One loader, shared with the `debug_gloves`
/// harness, so the two can't end up on different textures.
pub fn load_glove_texture(
    asset_cache: &mut AssetCache,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    asset_cache
        .get_opt::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, GLOVE_TEXTURE)
        .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the two constants against drifting apart: the scale exists to put
    /// the authored glove at a real hand's length once the world's true scale
    /// (`METERS_PER_WORLD_UNIT`) is applied.
    #[test]
    fn glove_scale_renders_a_life_size_hand() {
        let rendered_meters =
            AUTHORED_HAND_LENGTH_WORLD * GLOVE_SCALE * crate::METERS_PER_WORLD_UNIT;

        assert!(
            (rendered_meters - REAL_HAND_LENGTH_METERS).abs() < 1e-4,
            "glove renders {rendered_meters} m, expected {REAL_HAND_LENGTH_METERS} m"
        );
    }

    /// The uncorrected glove is undersized, not oversized - a scale below 1.0
    /// would mean a unit slip somewhere rather than a real correction.
    #[test]
    fn glove_scale_is_a_modest_enlargement() {
        assert!(
            (1.0..2.0).contains(&GLOVE_SCALE),
            "unexpected glove scale {GLOVE_SCALE}"
        );
    }
}
