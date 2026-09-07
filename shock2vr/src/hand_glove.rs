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
use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{EuclideanSpace, Matrix4, Point3, Quaternion, Transform, Vector3};
use dark::{
    glb_model::GlbModel,
    importers::{GLB_MODELS_IMPORTER, TEXTURE_IMPORTER, VR_CONTACT_MESH_IMPORTER, VrContactMesh},
};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{Material, SceneObject, SkinnedMaterial},
};

use crate::{
    hand_fit::{self, Capsule, ContactMesh, GripFamily, HandRig},
    hand_pose::{self, Finger, FingerAmounts, HandPoseRetarget, Pose},
    vr_config::{self, Handedness},
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

/// What a hand is holding, as far as its pose is concerned: nothing, or an
/// item - with the grip fitted to that item's own mesh when there was one to
/// fit against.
#[derive(Debug, Clone, Copy)]
pub enum Hold {
    Empty,
    Item(Option<FingerAmounts>),
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

/// Lean an analog-posed hand toward the prompt's authored shape - the same
/// `fist` and `point` poses the rest of the glove uses, so there is one
/// vocabulary of hand shapes rather than a second, synthetic one.
///
/// Trigger authority: `fist` is the maximal curl the analog blend already runs
/// toward, so leaning into it only ever adds curl; `point` is applied to every
/// finger *but* the index, which is what makes it a point and what leaves the
/// trigger's own finger exactly where the player put it.
fn prompted_pose(posed: Pose, fist: &Pose, point: &Pose, preshape: HandPreshape) -> Pose {
    match preshape {
        HandPreshape::None => posed,
        HandPreshape::Grip(weight) => posed.blend(fist, weight.clamp(0.0, 1.0)),
        HandPreshape::Point(weight) => {
            let weight = weight.clamp(0.0, 1.0);
            posed.blend_per_finger(
                point,
                &FingerAmounts {
                    thumb: weight,
                    index: 0.0,
                    middle: weight,
                    ring: weight,
                    pinky: weight,
                },
            )
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
    /// Solved grips, one per [`GripKey`] - `None` for a model with no mesh to
    /// fit against. The fit is a one-off per held model, never a per-frame
    /// cost.
    fits: HashMap<GripKey, Option<FittedGrip>>,
    /// The [`crate::vr_grips::generation`] `fits` was solved against.
    fits_generation: u64,
    /// Grips a *support* hand solved, keyed by the item and where on it the
    /// hand latched (see [`crate::two_hand_grip`]). Separate from `fits`
    /// because the same model gives a different answer at every grip point:
    /// a shotgun's pump closes the hand, its receiver barely does.
    support_fits: HashMap<(GripKey, [i32; 3]), Option<FittedGrip>>,
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
            fits: HashMap::new(),
            support_fits: HashMap::new(),
            fits_generation: crate::vr_grips::generation(),
        })
    }

    /// Build the posed glove scene objects for one hand.
    ///
    /// `hand_to_world` places the glove's own hand space - wrist at the origin,
    /// fingers down -Z. For a free or item-holding hand that is
    /// [`hand_to_world`] of the tracked pose; a hand wielding a gun composes
    /// `vr_config::held_gun_glove_seat` onto it, which seats the glove on the
    /// weapon in place of the baked hand the wield stripped off (and carries
    /// its own reflection, so pass [`Handedness::Right`] there).
    pub fn render_hand(
        &mut self,
        hand_to_world: Matrix4<f32>,
        trigger_value: f32,
        squeeze_value: f32,
        hold: Hold,
        light: HandLight,
        preshape: HandPreshape,
    ) -> Vec<SceneObject> {
        let amounts = if let Hold::Item(fit) = hold {
            grip_amounts(fit, trigger_value)
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
        let pose = self.open.blend_per_finger(&self.fist, &amounts);
        // A full hand has nothing to reach for, so it is never prompted.
        let pose = if matches!(hold, Hold::Item(_)) {
            pose
        } else {
            prompted_pose(pose, &self.fist, &self.point, preshape)
        };
        Self::render_posed(
            &mut self.model,
            &self.retarget,
            &self.materials[light.index()],
            &pose,
            hand_to_world,
        )
    }

    /// The fitted grip for a model held in `handedness`, solved once and
    /// remembered - the *miss* included, so a held model with no mesh does not
    /// re-enter the asset cache's miss path on every frame it is held.
    ///
    /// Solved on the frame the item first appears in a hand rather than at the
    /// grab itself: the answer depends only on the key, so one entry serves
    /// every later pickup of the same thing in the same hand.
    pub fn fitted_grip(
        &mut self,
        model_name: &str,
        handedness: Handedness,
        gun_scale: f32,
        asset_cache: &mut AssetCache,
    ) -> Option<FittedGrip> {
        // A tuner edit re-seats the item, so everything solved against the old
        // seat is stale. Cheaper to drop the lot than to track what moved.
        let generation = crate::vr_grips::generation();
        if self.fits_generation != generation {
            self.fits.clear();
            self.fits_generation = generation;
        }
        let key = GripKey::new(model_name, handedness, gun_scale);
        if let Some(cached) = self.fits.get(&key) {
            return *cached;
        }
        let fitted = self.solve(model_name, handedness, gun_scale, asset_cache);
        self.fits.insert(key, fitted);
        fitted
    }

    /// The grip a *support* hand closes in on an item the other hand holds.
    ///
    /// Same contact fit as a held item's, but the item is placed at the point
    /// this hand latched rather than at its own authored seat - the support
    /// hand did not pick the item up, it took hold of it somewhere.
    ///
    /// Cached per grip point rounded to the centimetre: finer is below what a
    /// tracked hand resolves, and would re-solve every frame.
    pub fn support_grip(
        &mut self,
        model_name: &str,
        handedness: Handedness,
        gun_scale: f32,
        latch: Vector3<f32>,
        asset_cache: &mut AssetCache,
    ) -> Option<FittedGrip> {
        let generation = crate::vr_grips::generation();
        if self.fits_generation != generation {
            self.fits.clear();
            self.support_fits.clear();
            self.fits_generation = generation;
        }
        let key = (
            GripKey::new(model_name, handedness, gun_scale),
            crate::two_hand_grip::quantise_latch(latch),
        );
        if let Some(cached) = self.support_fits.get(&key) {
            return *cached;
        }
        let fitted = self.solve_support(model_name, handedness, gun_scale, latch, asset_cache);
        self.support_fits.insert(key, fitted);
        fitted
    }

    fn solve_support(
        &mut self,
        model_name: &str,
        handedness: Handedness,
        gun_scale: f32,
        latch: Vector3<f32>,
        asset_cache: &mut AssetCache,
    ) -> Option<FittedGrip> {
        use cgmath::{EuclideanSpace, Point3, Transform};

        let triangles = asset_cache.get_opt::<_, VrContactMesh, _>(
            &VR_CONTACT_MESH_IMPORTER,
            &format!("{model_name}.BIN"),
        )?;
        // Slide the item along until the latched point sits on the palm; its
        // turn in the hand is the one the model's own grip already gives, which
        // is close enough at a support grip and keeps the answer cacheable.
        let seat = vr_config::held_model_hand_transform(model_name, handedness, gun_scale);
        let place = Matrix4::from_translation(
            crate::hand_seat::PALM_CENTRE - seat.transform_point(Point3::from_vec(latch)).to_vec(),
        ) * seat;
        let mesh = ContactMesh::new(
            triangles
                .0
                .iter()
                .map(|triangle| triangle.map(|corner| place.transform_point(corner)))
                .collect(),
        );
        if mesh.is_empty() {
            return None;
        }

        // The support hand has no trigger to rest on, so a gun's authored
        // Trigger family is not its family. An authored support seat may name
        // one; otherwise the item's own girth decides, which is what makes a
        // basketball Broad and a rifle handguard Cylindrical.
        let family = crate::vr_grips::support_seats(model_name)
            .first()
            .and_then(|seat| seat.family())
            .or_else(|| mesh.extents().map(hand_fit::family_from_extents))
            .unwrap_or(GripFamily::Cylindrical);
        let mut rig = GloveRig {
            model: &mut self.model,
            retarget: &self.retarget,
            open: &self.open,
            fist: &self.fist,
            to_hand: glove_model_to_hand(),
        };
        let amounts = hand_fit::fit(&mut rig, &mesh, family, unmeasured_wrap(family));
        Some(FittedGrip { family, amounts })
    }

    /// One grip fit, start to finish. Only ever reached on a cache miss.
    fn solve(
        &mut self,
        model_name: &str,
        handedness: Handedness,
        gun_scale: f32,
        asset_cache: &mut AssetCache,
    ) -> Option<FittedGrip> {
        let grip = crate::vr_grips::resolve(model_name, handedness);
        if grip.source == crate::vr_grips::GripSource::Unseated {
            return None;
        }

        // An authored per-finger curl is the last word - the escape hatch for a
        // shape the contact sweep reads wrong - so it never pays for the mesh.
        if let (Some(amounts), Some(family)) = (grip.fingers, grip.family) {
            return Some(FittedGrip { family, amounts });
        }

        let mesh = contact_mesh(model_name, handedness, gun_scale, asset_cache)?;
        let family = grip
            .family
            .or_else(|| mesh.extents().map(hand_fit::family_from_extents))?;

        let started = std::time::Instant::now();
        let mut rig = GloveRig {
            model: &mut self.model,
            retarget: &self.retarget,
            open: &self.open,
            fist: &self.fist,
            to_hand: glove_model_to_hand(),
        };
        let amounts = hand_fit::fit(&mut rig, &mesh, family, unmeasured_wrap(family));
        let solve = started.elapsed();

        tracing::debug!(
            "grip fit: {model_name} {handedness:?} x{gun_scale} family {} {} tris solved in {solve:?} -> {amounts:?}",
            family.as_str(),
            mesh.triangle_count()
        );
        Some(FittedGrip { family, amounts })
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
            hand_to_world(position, rotation, handedness),
        )
    }

    /// Bake `pose` into the shared model and place it at `hand_to_world` - the
    /// transform of the glove's own hand space, wrist at the origin with the
    /// fingers down -Z.
    ///
    /// The one place the glove-model-to-hand-space conversion lives, so every
    /// caller gets the same hand at the same place.
    fn render_posed(
        model: &mut GlbModel,
        retarget: &HandPoseRetarget,
        skin: &[Rc<RefCell<Box<dyn Material>>>],
        pose: &Pose,
        hand_to_world: Matrix4<f32>,
    ) -> Vec<SceneObject> {
        retarget.apply(pose, model);

        // The glove's fingers point along the model's +Z; the hand frame's
        // forward is -Z (the raycast/aim direction, see VirtualHand::update),
        // so the grip alignment yaws the model 180 degrees to line the
        // fingers up with where the hand points. Verified against the
        // raycast hit markers in-game; on-headset fine tuning would adjust
        // this rotation.
        let world = hand_to_world * glove_model_to_hand();

        let mut objects = model.to_scene_objects_with_skinning();
        for (object, material) in objects.iter_mut().zip(skin) {
            object.material = material.clone();
            object.set_transform(world);
        }

        objects
    }
}

/// Thickness of a finger's phalanx capsule, in world units - the flesh the
/// fit stops on rather than the bone the rig gives it.
const FINGER_RADIUS: f32 = 0.0075 / crate::METERS_PER_WORLD_UNIT;

/// The thumb is thicker than the fingers, and lands flatter on an item.
const THUMB_RADIUS: f32 = 0.0095 / crate::METERS_PER_WORLD_UNIT;

/// The joints a finger's phalanx capsules run between, knuckle to tip. The
/// metacarpal - the first bone of the chain - is buried in the palm and cannot
/// touch anything, so every chain starts one bone in.
fn phalanx_bones(finger: Finger) -> impl Iterator<Item = usize> {
    finger.bones().skip(1)
}

/// Glove model space -> hand space: the same seat [`GloveRenderer::render_posed`]
/// draws the glove at, so the fit measures the hand the player actually sees.
fn glove_model_to_hand() -> Matrix4<f32> {
    Matrix4::from_angle_y(cgmath::Deg(180.0)) * Matrix4::from_scale(GLOVE_SCALE)
}

/// The glove as the finger fit poses it: one finger curled from `open` toward
/// `fist`, its phalanges read straight back out of the rig.
struct GloveRig<'a> {
    model: &'a mut GlbModel,
    retarget: &'a HandPoseRetarget,
    open: &'a Pose,
    fist: &'a Pose,
    to_hand: Matrix4<f32>,
}

impl HandRig for GloveRig<'_> {
    fn phalanges(&mut self, finger: Finger, curl: f32) -> Vec<Capsule> {
        let pose = self.open.blend_per_finger(self.fist, &finger.alone(curl));
        self.retarget.apply(&pose, self.model);

        let radius = if finger == Finger::Thumb {
            THUMB_RADIUS
        } else {
            FINGER_RADIUS
        };
        let joints = phalanx_bones(finger)
            .filter_map(|joint| {
                let node = self.model.skeleton().node_index_for_joint(joint)?;
                let global = self.model.get_global_transform(node)?;
                Some(
                    self.to_hand
                        .transform_point(Point3::from_vec(global.w.truncate())),
                )
            })
            .collect::<Vec<_>>();

        joints
            .windows(2)
            .map(|bone| Capsule {
                a: bone[0],
                b: bone[1],
                radius,
            })
            .collect()
    }
}

/// What a fitted grip is cached under: the model, which hand's reflection it
/// is seen through, and the scale the wield baked into its geometry. Those
/// three decide everything else - the family included - so the key can be
/// built without touching the mesh, which is what keeps a held item's every
/// later frame off the solver.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GripKey {
    model: String,
    handedness: Handedness,
    /// `f32` has no `Hash`; the bit pattern is the exact-equality the cache
    /// wants anyway - a different scale is a different solve.
    scale_bits: u32,
}

impl GripKey {
    pub fn new(model: &str, handedness: Handedness, scale: f32) -> Self {
        Self {
            model: model.to_ascii_lowercase(),
            handedness,
            scale_bits: scale.to_bits(),
        }
    }
}

/// A hand's grip on one item: the family it was solved in, and how far each
/// finger curled from `open` toward `fist` before it reached the surface.
#[derive(Clone, Copy, Debug)]
pub struct FittedGrip {
    pub family: GripFamily,
    pub amounts: FingerAmounts,
}

/// Measure `model_name`'s seat off its own geometry, if it still needs one.
///
/// The placement path ([`vr_config::get_vr_hand_model_adjustments_from_entity`],
/// called while the hands update) has no asset cache, so the answer is measured
/// here - where the mesh is reachable - and left in [`crate::vr_grips`] for it
/// to read. The mission loop calls this for whatever the hands already hold, so
/// an item is seated on the frame *after* the one it was grabbed on; the first
/// frame of a grab draws it on the wrist, as every frame used to.
///
/// A model with nothing to measure (a skinned melee rig, a missing `.BIN`)
/// records that fact rather than nothing: the asset cache memoizes only
/// successes, so an unremembered miss re-walks every mount, every frame the
/// item is held.
pub fn warm_held_seat(model_name: &str, asset_cache: &mut AssetCache) {
    if !crate::vr_grips::needs_measurement(model_name) {
        return;
    }
    // The box of the geometry as it is *drawn*: the held scale is part of how
    // deep the item sits in the palm.
    let scale = vr_config::held_geometry_scale(model_name);
    let measured = asset_cache
        .get_opt::<_, VrContactMesh, _>(&VR_CONTACT_MESH_IMPORTER, &format!("{model_name}.BIN"))
        .and_then(|triangles| hand_fit::triangle_bounds(&triangles.0));

    match measured {
        Some((min, max)) => {
            crate::vr_grips::remember_measured_seat(model_name, min * scale, max * scale)
        }
        None => crate::vr_grips::remember_unmeasurable(model_name),
    }
}

/// The item's render mesh in the glove's own hand space, or `None` when there
/// is nothing to close against - a missing asset, or a skinned rig (the melee
/// `_h` set, which draws its own arm anyway).
fn contact_mesh(
    model_name: &str,
    handedness: Handedness,
    gun_scale: f32,
    asset_cache: &mut AssetCache,
) -> Option<ContactMesh> {
    let triangles = asset_cache
        .get_opt::<_, VrContactMesh, _>(&VR_CONTACT_MESH_IMPORTER, &format!("{model_name}.BIN"))?;

    let to_hand = vr_config::held_model_hand_transform(model_name, handedness, gun_scale);
    let mesh = ContactMesh::new(
        triangles
            .0
            .iter()
            .map(|triangle| triangle.map(|corner| to_hand.transform_point(corner)))
            .collect(),
    );
    (!mesh.is_empty()).then_some(mesh)
}

/// Where a tracked hand's own space sits in the world.
///
/// The right-hand model is mirrored across the hand's local X for the left hand
/// - `Handedness::mirror`, the one definition of "the other hand", shared with
/// the melee wield so the glove and the arm rig cannot disagree about which way
/// round the left hand is.
pub fn hand_to_world(
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    handedness: Handedness,
) -> Matrix4<f32> {
    Matrix4::from_translation(position) * Matrix4::from(rotation) * handedness.mirror()
}

/// The curl of a hand closed around something it is holding: the grip fitted
/// to that item's own mesh when there is one, else the generic wrap below.
///
/// The trigger always adds curl on top of the index, from wherever the fit
/// left it: a real pull outranks a fitted rest position, the same way it
/// outranks a pre-shape. The squeeze is what holds the item, so it does not
/// drive the pose here.
fn grip_amounts(fit: Option<FingerAmounts>, trigger_value: f32) -> FingerAmounts {
    let base = fit.unwrap_or(GENERIC_GRIP);
    FingerAmounts {
        index: base.index + (1.0 - base.index) * trigger_value,
        ..base
    }
}

/// The wrap a hand falls back on when the item has no mesh to fit against.
/// Constants tuned visually against the held pistol.
const GENERIC_GRIP: FingerAmounts = FingerAmounts {
    thumb: 0.85,
    index: 0.5,
    middle: 0.9,
    ring: 0.9,
    pinky: 0.9,
};

/// Where a finger the contact fit could not measure is left.
///
/// The fit yields nothing for a finger already touching the item at the open
/// pose, and what that *means* depends on what the hand is holding. A gun's
/// grip is seated inside the closed hand on purpose, so its fingers start
/// inside the mesh and the generic wrap is the answer. Everything else is
/// seated against the **open** palm on purpose ([`crate::hand_seat`]), so a
/// finger already touching has finished closing - the thumb lies on anything
/// the palm carries - and curling it on would drive it through.
///
/// Keyed on the family, not on where the seat came from: `Trigger` is the one
/// family nothing measures its way into, and authoring a pickup's offset in
/// the tuner must not silently change how its fingers land.
fn unmeasured_wrap(family: GripFamily) -> FingerAmounts {
    match family {
        GripFamily::Trigger => GENERIC_GRIP,
        _ => FingerAmounts::default(),
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

/// The palm frame of the open glove, in the glove's own hand space: where the
/// palm's centre is, which way it faces, which way the fingers curl about, and
/// where thumb and index meet.
///
/// [`crate::hand_seat`] seats unprofiled items against this. It is a constant
/// of the rig rather than something a seat can afford to pose a glove for, so
/// that module writes the numbers down and
/// `the_palm_frame_matches_the_glove_rig` holds them to this measurement.
#[cfg(test)]
fn palm_frame_of(
    model: &mut GlbModel,
    retarget: &HandPoseRetarget,
    open: &Pose,
    fist: &Pose,
) -> (Vector3<f32>, Vector3<f32>, Vector3<f32>, Vector3<f32>) {
    use cgmath::{InnerSpace, Zero};

    retarget.apply(open, model);
    let to_hand = glove_model_to_hand();
    let joint = |model: &mut GlbModel, index: usize| {
        model
            .skeleton()
            .node_index_for_joint(index)
            .and_then(|node| model.get_global_transform(node))
            .map(|global| {
                to_hand
                    .transform_point(Point3::from_vec(global.w.truncate()))
                    .to_vec()
            })
            .unwrap_or_else(|| panic!("the glove rig has no joint {index}"))
    };
    // The palm's flat is the quad of the four fingers' metacarpals: base and
    // knuckle of each. The thumb is left out - it swings out of that plane.
    let fingers = [Finger::Index, Finger::Middle, Finger::Ring, Finger::Pinky];
    let mut bases = Vector3::zero();
    let mut knuckles = Vector3::zero();
    for finger in fingers {
        let mut bones = finger.bones();
        bases += joint(model, *bones.start());
        knuckles += joint(model, bones.nth(1).expect("a finger has a knuckle"));
    }
    let (bases, knuckles) = (bases * 0.25, knuckles * 0.25);

    let centre = (bases + knuckles) * 0.5;
    let curl = (joint(model, *Finger::Pinky.bones().nth(1).as_ref().unwrap())
        - joint(model, *Finger::Index.bones().nth(1).as_ref().unwrap()))
    .normalize();
    // Out of the palm is what the fingers curl toward, which the wrist-to-
    // knuckle run and the across-the-palm run bracket in this order.
    let normal = (knuckles - bases).cross(curl).normalize();
    // Where thumb and index pads converge. Searched over the pair's curl
    // range, not read off the open pose: open, the two are 7 cm apart and one
    // behind the other, and their tips only come face to face part way closed.
    let mut rig = GloveRig {
        model,
        retarget,
        open,
        fist,
        to_hand,
    };
    let mut tip = |rig: &mut GloveRig, finger| {
        // Posed once per curl per finger, not once per pair: the index's tip
        // does not depend on where the thumb is, and posing the rig is what
        // this costs.
        (0..=PINCH_STEPS)
            .map(|i| {
                rig.phalanges(finger, i as f32 / PINCH_STEPS as f32)
                    .last()
                    .map(|capsule: &Capsule| capsule.b.to_vec())
                    .expect("a finger has phalanges")
            })
            .collect::<Vec<_>>()
    };
    const PINCH_STEPS: usize = 20;
    let thumb_tips = tip(&mut rig, Finger::Thumb);
    let index_tips = tip(&mut rig, Finger::Index);
    let mut pinch = (f32::INFINITY, Vector3::zero());
    for &thumb in &thumb_tips {
        for &index in &index_tips {
            let gap = (thumb - index).magnitude();
            if gap < pinch.0 {
                pinch = (gap, (thumb + index) * 0.5);
            }
        }
    }
    (centre, normal, curl, pinch.1)
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
    use crate::util::meters;

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

    /// The glove built for a fit, from the shipped rig and nothing else.
    #[cfg(test)]
    fn test_glove() -> (GlbModel, HandPoseRetarget) {
        use collision::Aabb3;
        use dark::importers::skeleton_from_glb_bytes;

        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/",
            "vr_glove_model.glb"
        );
        let bytes = std::fs::read(path).expect("read the glove GLB");
        let skeleton = skeleton_from_glb_bytes(&bytes).expect("the glove GLB has a skeleton");
        let unit = Aabb3::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0));
        let model = GlbModel::new(Vec::new(), unit, skeleton);
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        (model, retarget)
    }

    /// `hand_seat` writes the palm frame down rather than posing a glove to
    /// find it, so re-measure it off the shipped rig here: an unprofiled item
    /// is seated against this frame, and a glove that moved under it would seat
    /// every one of them somewhere the fingers are not.
    #[test]
    fn the_palm_frame_matches_the_glove_rig() {
        use cgmath::InnerSpace;

        let (mut model, retarget) = test_glove();

        let (centre, normal, curl, pinch) = palm_frame_of(
            &mut model,
            &retarget,
            &hand_pose::open_right_hand(),
            &hand_pose::fist_right_hand(),
        );

        for (name, measured, written) in [
            ("centre", centre, crate::hand_seat::PALM_CENTRE),
            ("normal", normal, crate::hand_seat::PALM_NORMAL),
            ("curl axis", curl, crate::hand_seat::PALM_CURL_AXIS),
            ("pinch point", pinch, crate::hand_seat::PINCH_POINT),
        ] {
            assert!(
                (measured - written).magnitude() < 2e-3,
                "glove rig palm {name} is {measured:?}, hand_seat says {written:?}"
            );
        }
    }

    /// Only a gun keeps the canned wrap. Everything else is seated against the
    /// open palm, so an unmeasurable finger stays where it is - and because
    /// this reads the family rather than the seat's provenance, authoring a
    /// pickup's offset in the tuner cannot change how its fingers land.
    #[test]
    fn only_a_trigger_grip_falls_back_on_the_canned_wrap() {
        for family in GripFamily::ALL {
            let wrap = unmeasured_wrap(family);
            if family == GripFamily::Trigger {
                assert_eq!(wrap.thumb, GENERIC_GRIP.thumb);
            } else {
                assert_eq!(
                    (wrap.thumb, wrap.index, wrap.middle, wrap.ring, wrap.pinky),
                    (0.0, 0.0, 0.0, 0.0, 0.0),
                    "{} should leave an unmeasured finger open",
                    family.as_str()
                );
            }
        }
    }

    /// The whole point of the seat: a handle put against the open palm is
    /// somewhere the fingers can *reach and stop on*, so the fit measures a
    /// real wrap instead of falling back on a canned one.
    ///
    /// Negative test: seat the same cylinder the way #1385 did - dropped along
    /// hand -Y, which runs across the knuckles rather than out of the palm, so
    /// the item lands beside the hand - and every finger runs to its cap
    /// without ever meeting it.
    ///
    /// The thumb is the deliberate exception: it lies *on* whatever the open
    /// palm carries, so it is at contact before it curls at all. `solve` gives
    /// a measured seat the open hand as its fallback for exactly that reason,
    /// which is what this asserts for the thumb.
    #[test]
    fn a_seated_handle_is_somewhere_every_finger_can_close_onto_it() {
        use crate::hand_fit::{ContactMesh, GripFamily};
        use crate::hand_pose::Finger;

        // A 40 mm handle, 150 mm long, drawn centred on the model origin the
        // way a world pickup is.
        let radius = meters(0.02);
        let half_length = meters(0.075);
        let half = Vector3::new(half_length, radius, radius);
        let (seat, family) = crate::hand_seat::seat(-half, half, None);
        assert_eq!(family, GripFamily::Cylindrical);

        let mesh = ContactMesh::new(seated_cylinder(radius, half_length, seat));
        let (mut model, retarget) = test_glove();
        let open = hand_pose::open_right_hand();
        let fist = hand_pose::fist_right_hand();
        let mut rig = GloveRig {
            model: &mut model,
            retarget: &retarget,
            open: &open,
            fist: &fist,
            to_hand: glove_model_to_hand(),
        };

        // Fitted twice against opposite fallbacks: a finger that answers the
        // same either way measured the item, and one that just echoes the
        // fallback did not.
        let amounts = hand_fit::fit(&mut rig, &mesh, family, all_fingers(0.0));
        let against_a_fist = hand_fit::fit(&mut rig, &mesh, family, all_fingers(1.0));

        for finger in [Finger::Index, Finger::Middle, Finger::Ring, Finger::Pinky] {
            let curl = finger.curl_of(&amounts);
            assert!(
                (curl - finger.curl_of(&against_a_fist)).abs() < 1e-6,
                "{finger:?} answered the fallback, not the item"
            );
            assert!(
                curl > 0.0 && curl < 1.0,
                "{finger:?} closed to {curl} without stopping on the handle"
            );
        }
        assert!(
            Finger::Thumb.curl_of(&amounts) == 0.0 && Finger::Thumb.curl_of(&against_a_fist) == 1.0,
            "the thumb should be reading its fallback - it starts resting on the handle"
        );
    }

    #[cfg(test)]
    fn all_fingers(curl: f32) -> FingerAmounts {
        FingerAmounts {
            thumb: curl,
            index: curl,
            middle: curl,
            ring: curl,
            pinky: curl,
        }
    }

    /// The cylinder of `a_seated_handle_is_somewhere_every_finger_can_close_onto_it`,
    /// in hand space: model-space triangles put through the seat.
    #[cfg(test)]
    fn seated_cylinder(
        radius: f32,
        half_length: f32,
        seat: crate::hand_seat::Seat,
    ) -> Vec<[Point3<f32>; 3]> {
        use cgmath::Rotation;

        const SEGMENTS: usize = 48;
        let place =
            |v: Vector3<f32>| Point3::from_vec(seat.rotation.rotate_vector(v) + seat.offset);
        let ring = |i: usize| {
            let angle = std::f32::consts::TAU * (i as f32) / (SEGMENTS as f32);
            (radius * angle.cos(), radius * angle.sin())
        };
        let mut triangles = Vec::new();
        for i in 0..SEGMENTS {
            let (y0, z0) = ring(i);
            let (y1, z1) = ring(i + 1);
            let quad = [
                Vector3::new(-half_length, y0, z0),
                Vector3::new(half_length, y0, z0),
                Vector3::new(half_length, y1, z1),
                Vector3::new(-half_length, y1, z1),
            ]
            .map(place);
            triangles.push([quad[0], quad[1], quad[2]]);
            triangles.push([quad[0], quad[2], quad[3]]);
        }
        triangles
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

    /// Slerping a pose onto itself is exact only up to float round-off.
    fn same_rotation(a: cgmath::Quaternion<f32>, b: cgmath::Quaternion<f32>) -> bool {
        use cgmath::InnerSpace;
        (a - b).magnitude() < 1e-4
    }

    /// A trigger pull owns the index finger: the point prompt curls the other
    /// four toward the authored pointing pose and leaves the index exactly
    /// where the player put it, and no prompt at all changes nothing.
    #[test]
    fn the_point_prompt_leaves_the_pulled_index_alone() {
        let open = hand_pose::open_right_hand();
        let fist = hand_pose::fist_right_hand();
        let point = hand_pose::point_right_hand();
        let pulled = open.blend_per_finger(
            &fist,
            &FingerAmounts {
                index: 1.0,
                ..FingerAmounts::default()
            },
        );

        let prompted = prompted_pose(pulled.clone(), &fist, &point, HandPreshape::Point(0.4));
        // SteamVR bones 6..=10 are the index chain.
        for bone in 6..=10 {
            assert!(
                same_rotation(prompted.bone_rotations[bone], pulled.bone_rotations[bone]),
                "the point prompt must not move index bone {bone}"
            );
        }
        let moved = (11..=15)
            .any(|bone| !same_rotation(prompted.bone_rotations[bone], pulled.bone_rotations[bone]));
        assert!(moved, "the point prompt should curl the middle finger");

        let unprompted = prompted_pose(pulled.clone(), &fist, &point, HandPreshape::None);
        assert_eq!(unprompted.bone_rotations, pulled.bone_rotations);
    }

    /// The grip prompt only ever adds curl - it leans toward the same fist the
    /// analog blend already runs toward, so a fully squeezed hand is unmoved.
    #[test]
    fn the_grip_prompt_never_uncurls_a_closed_hand() {
        let fist = hand_pose::fist_right_hand();
        let point = hand_pose::point_right_hand();

        let prompted = prompted_pose(fist.clone(), &fist, &point, HandPreshape::Grip(0.4));
        for (bone, (prompted, closed)) in prompted
            .bone_rotations
            .iter()
            .zip(&fist.bone_rotations)
            .enumerate()
        {
            assert!(
                same_rotation(*prompted, *closed),
                "the grip prompt moved bone {bone} of an already closed hand"
            );
        }
    }

    /// Everything the solve depends on is in the key, and nothing else is: the
    /// same model in the other hand or at another scale is a different fit and
    /// must not be served the cached one - while the same model spelled
    /// differently is the same fit.
    #[test]
    fn a_grip_is_cached_by_what_the_solve_depends_on() {
        let base = GripKey::new("atek_h", Handedness::Right, 0.4);

        assert_eq!(base, GripKey::new("ATEK_H", Handedness::Right, 0.4));
        for other in [
            GripKey::new("sg_h", Handedness::Right, 0.4),
            GripKey::new("atek_h", Handedness::Left, 0.4),
            GripKey::new("atek_h", Handedness::Right, 1.0),
        ] {
            assert_ne!(base, other, "{other:?} must not reuse {base:?}");
        }
    }

    /// A trigger pull only ever adds curl to the index, from wherever the fit
    /// left it - and touches nothing else.
    #[test]
    fn a_trigger_pull_closes_the_fitted_index_the_rest_of_the_way() {
        let fitted = FingerAmounts {
            thumb: 0.7,
            index: 0.3,
            middle: 0.6,
            ring: 0.6,
            pinky: 0.6,
        };

        let resting = grip_amounts(Some(fitted), 0.0);
        assert_eq!(resting.index, fitted.index);
        assert_eq!(resting.middle, fitted.middle);

        let pulled = grip_amounts(Some(fitted), 1.0);
        assert_eq!(pulled.index, 1.0);
        assert_eq!(pulled.middle, fitted.middle);
        assert_eq!(pulled.thumb, fitted.thumb);

        // No fit: the generic wrap, still with the trigger on top.
        assert_eq!(grip_amounts(None, 0.0).index, GENERIC_GRIP.index);
        assert_eq!(grip_amounts(None, 1.0).index, 1.0);
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
