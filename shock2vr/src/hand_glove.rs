//! Renders the SteamVR glove model as the player's VR hands, posed from the
//! controller's analog inputs (trigger curls the index finger, squeeze curls
//! the rest). The left hand mirrors the right-hand model (`flip_x`-style
//! negative scale), like held-weapon models do.
//!
//! The glove wears its own colour map plus a light: an emissive mask marks the
//! cuff band and the fingertips, and a per-hand [`HandLight`] tint decides what
//! colour they glow. The hand ends at the glove's own cuff - there is no
//! forearm mesh.

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

/// The glove's own colour map, as the model ships it.
const GLOVE_COLOR_TEXTURE: &str = "vr_glove_color.jpg";

/// Where the glove's light sits, in that same UV atlas: white texels glow,
/// black ones don't. A band round the wrist cuff and a pad on each fingertip
/// carry the front; the stripes down the back-of-hand panel carry the back, so
/// the light reads from whichever side of the hand faces the player.
///
/// The PNG is hand-authored; `tools/make_vr_glove_emissive.py` only painted its
/// first draft.
const GLOVE_EMISSIVE_TEXTURE: &str = "vr_glove_emissive.png";

/// The affordance light on a hand's glove: what the hand in front of you could
/// do right now, read off the glove itself rather than a marker floating in the
/// world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandLight {
    /// Nothing in reach.
    Off,
    /// Something grabbable.
    Green,
    /// Something usable, or a gesture that didn't take.
    Amber,
    /// Locked, or otherwise refused.
    Red,
}

impl HandLight {
    /// Every state, in declaration order. The renderer indexes its materials by
    /// position here, and a harness showing them all reads the same list, so
    /// neither can fall behind a new variant.
    pub const ALL: [HandLight; 4] = [
        HandLight::Off,
        HandLight::Green,
        HandLight::Amber,
        HandLight::Red,
    ];

    /// What the lit texels are added in. The shader *adds* this on top of the
    /// glove's own shading, so these run bright enough to read against a lit
    /// cuff without washing out to white.
    pub fn tint(self) -> Vector3<f32> {
        match self {
            HandLight::Off => Vector3::new(0.0, 0.0, 0.0),
            HandLight::Green => Vector3::new(0.05, 0.85, 0.25),
            HandLight::Amber => Vector3::new(0.90, 0.55, 0.05),
            HandLight::Red => Vector3::new(0.90, 0.10, 0.10),
        }
    }

    /// Its slot in [`HandLight::ALL`] - the discriminant, which
    /// `every_hand_light_indexes_its_own_slot` holds to declaration order.
    fn index(self) -> usize {
        self as usize
    }
}

/// A prompt the hand leans into before the player acts: the shape says what
/// the action would be, the weight (0..1) how far to lean. Applied as a *floor*
/// on the analog curl, so a real trigger or squeeze always outranks it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandPreshape {
    /// Nothing to prompt.
    None,
    /// Curl toward a grip - something to pick up.
    Grip(f32),
    /// Curl the rest, leave the index extended - something to press.
    Point(f32),
}

impl HandPreshape {
    /// Lean the analog curl toward this shape. Never overrides: a finger the
    /// player is already curling further stays where they put it.
    fn apply(self, amounts: &mut FingerAmounts) {
        let (weight, index_too) = match self {
            HandPreshape::None => return,
            HandPreshape::Grip(weight) => (weight, true),
            HandPreshape::Point(weight) => (weight, false),
        };
        let weight = weight.clamp(0.0, 1.0);
        amounts.thumb = amounts.thumb.max(weight);
        amounts.middle = amounts.middle.max(weight);
        amounts.ring = amounts.ring.max(weight);
        amounts.pinky = amounts.pinky.max(weight);
        if index_too {
            amounts.index = amounts.index.max(weight);
        }
    }
}

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
/// blend endpoints, and one textured material per mesh per [`HandLight`] state
/// (fresh cells so the cached asset's materials are never mutated).
pub struct GloveRenderer {
    model: GlbModel,
    retarget: HandPoseRetarget,
    open: Pose,
    fist: Pose,
    point: Pose,
    /// Materials per mesh, indexed by [`HandLight::index`]. Both hands render
    /// in the same frame and can be showing different lights, so one set of
    /// materials re-tinted per hand would give them both whichever colour was
    /// written last.
    materials: Vec<Vec<Rc<RefCell<Box<dyn Material>>>>>,
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

        let color = load_glove_color(asset_cache);
        let emissive = load_glove_emissive(asset_cache);

        // One material per mesh per light state.
        let authored = model.to_scene_objects();
        let materials = HandLight::ALL
            .iter()
            .map(|light| {
                authored
                    .iter()
                    .map(|object| {
                        glove_material(&color, &emissive, *light)
                            .unwrap_or_else(|| object.material.clone())
                    })
                    .collect()
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

    /// Build the posed glove scene objects for one hand at its world transform.
    pub fn render_hand(
        &mut self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
        trigger_value: f32,
        squeeze_value: f32,
        holding: bool,
        light: HandLight,
        preshape: HandPreshape,
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
        // The prompt is a floor under the analog curl, so a real pull always
        // shows through it. A full hand has nothing to reach for.
        if !holding {
            preshape.apply(&mut amounts);
        }
        let pose = self.open.blend_per_finger(&self.fist, &amounts);
        Self::render_posed(
            &mut self.model,
            &self.retarget,
            &self.materials[light.index()],
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
            &self.materials[HandLight::Off.index()],
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
        skin: &[Rc<RefCell<Box<dyn Material>>>],
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
        let grip = Matrix4::from_angle_y(cgmath::Deg(180.0));
        let mirror = handedness.mirror();
        let world = Matrix4::from_translation(position)
            * Matrix4::from(rotation)
            * mirror
            * grip
            * Matrix4::from_scale(GLOVE_SCALE);

        let mut objects = model.to_scene_objects_with_skinning();
        for (object, material) in objects.iter_mut().zip(skin) {
            object.material = material.clone();
            object.set_transform(world);
        }

        objects
    }
}

/// One glove material, in a cell of its own. Shared with the `debug_gloves`
/// harness so the two can't assemble the glove differently.
///
/// `None` when there is no colour map - the caller then keeps the material the
/// importer built (solid-colour fallback). Without the light mask the glove
/// renders unlit rather than not at all.
///
/// A fresh cell every call is load-bearing: the model's own material is shared
/// by every copy of the glove in the scene, so writing a tint through it would
/// give both hands whichever light was set last.
pub fn glove_material(
    color: &Option<Rc<dyn engine::texture::TextureTrait>>,
    emissive: &Option<Rc<dyn engine::texture::TextureTrait>>,
    light: HandLight,
) -> Option<Rc<RefCell<Box<dyn Material>>>> {
    let color = color.as_ref()?;
    Some(Rc::new(RefCell::new(match emissive {
        Some(emissive) => SkinnedMaterial::create_with_light(
            color.clone(),
            1.0,
            0.0,
            emissive.clone(),
            light.tint(),
        ),
        None => SkinnedMaterial::create(color.clone(), 1.0, 0.0),
    })))
}

/// The glove's colour map. One loader, shared with the `debug_gloves` harness,
/// so the two can't end up on different textures.
pub fn load_glove_color(
    asset_cache: &mut AssetCache,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    load_texture(asset_cache, GLOVE_COLOR_TEXTURE)
}

/// The glove's light mask, shared with `debug_gloves` for the same reason.
pub fn load_glove_emissive(
    asset_cache: &mut AssetCache,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    load_texture(asset_cache, GLOVE_EMISSIVE_TEXTURE)
}

fn load_texture(
    asset_cache: &mut AssetCache,
    name: &str,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    asset_cache
        .get_opt::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, name)
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

    /// Only `Off` is dark, and no two lit states share a colour - a light the
    /// player can't tell from another one signals nothing.
    #[test]
    fn every_hand_light_has_its_own_visible_colour() {
        assert_eq!(HandLight::Off.tint(), Vector3::new(0.0, 0.0, 0.0));

        let lit: Vec<Vector3<f32>> = HandLight::ALL
            .iter()
            .filter(|light| **light != HandLight::Off)
            .map(|light| light.tint())
            .collect();

        for (index, tint) in lit.iter().enumerate() {
            use cgmath::InnerSpace;
            assert!(tint.magnitude() > 0.25, "{tint:?} is too dim to read");
            for other in &lit[index + 1..] {
                assert!((tint - other).magnitude() > 0.25, "{tint:?} ~ {other:?}");
            }
        }
    }

    /// Every variant must have a materials slot: `render_hand` indexes the
    /// renderer's per-light materials by this position.
    #[test]
    fn every_hand_light_indexes_its_own_slot() {
        for (expected, light) in HandLight::ALL.iter().enumerate() {
            assert_eq!(light.index(), expected);
        }
    }

    /// The prompt only ever adds curl, and the player's own pull outranks it:
    /// a full trigger stays full whatever the hand is hovering.
    #[test]
    fn preshape_floors_the_curl_without_overriding_a_real_pull() {
        let pulled = FingerAmounts {
            index: 1.0,
            ..FingerAmounts::default()
        };

        let mut grip = pulled;
        HandPreshape::Grip(0.3).apply(&mut grip);
        assert_eq!(grip.index, 1.0, "a real trigger pull must win");
        assert_eq!(grip.thumb, 0.3);
        assert_eq!(grip.pinky, 0.3);

        // Point leaves the index extended - that is what makes it a point.
        let mut point = FingerAmounts::default();
        HandPreshape::Point(0.3).apply(&mut point);
        assert_eq!(point.index, 0.0);
        assert_eq!(point.middle, 0.3);

        let mut none = pulled;
        HandPreshape::None.apply(&mut none);
        assert_eq!(none.thumb, 0.0);
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
