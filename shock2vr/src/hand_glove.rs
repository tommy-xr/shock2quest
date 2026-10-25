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
    hand_pose::{self, FingerAmounts, HandPoseRetarget, HandShape, Pose},
    vr_config::Handedness,
    vr_grip::{PHALANX_SAMPLES, Sphere},
};

const GLOVE_MODEL: &str = "vr_glove_model.glb";

/// Authored glove colour map, shared with the pose inspection scene.
const GLOVE_TEXTURE: &str = "vr_glove_color.jpg";
const GLOVE_EMISSIVE_TEXTURE: &str = "vr_glove_emissive.png";

/// The per-hand feedback colour, applied only to the authored emissive mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HandLight {
    #[default]
    Off,
    Green,
    Amber,
    Red,
    /// A climbable hold is within reach: the emissive lights glow half cyan.
    ClimbReach,
    /// The hand is holding a climbable hold: the emissive lights glow cyan.
    ClimbHeld,
}

impl HandLight {
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::Green,
        Self::Amber,
        Self::Red,
        Self::ClimbReach,
        Self::ClimbHeld,
    ];

    pub fn tint(self) -> Vector3<f32> {
        const CLIMB_CYAN: Vector3<f32> = Vector3::new(0.0, 0.7, 0.85);
        match self {
            Self::Off => Vector3::new(0.0, 0.0, 0.0),
            Self::Green => Vector3::new(0.05, 0.85, 0.25),
            Self::Amber => Vector3::new(0.90, 0.55, 0.05),
            Self::Red => Vector3::new(0.90, 0.10, 0.10),
            Self::ClimbReach => CLIMB_CYAN * 0.5,
            Self::ClimbHeld => CLIMB_CYAN,
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
/// blend endpoints, and one textured material per mesh (fresh cells so the
/// cached asset's materials are never mutated).
pub struct GloveRenderer {
    model: GlbModel,
    retarget: HandPoseRetarget,
    open: Pose,
    fist: Pose,
    point: Pose,
    wrist_frames: Option<[Matrix4<f32>; 2]>,
    open_joints: [Option<crate::hand_capture::HandJoints>; 2],
    /// Carries pose-number bone turns onto the glove model (`HandPoseRetarget::delta_basis`).
    delta_basis: cgmath::Matrix3<f32>,
    /// Measured once from the model's skin (`finger_spheres`).
    spheres: FingerSpheres,
    materials: Vec<Vec<Rc<RefCell<Box<dyn Material>>>>>,
    lit_materials: Vec<Vec<Rc<RefCell<Box<dyn Material>>>>>,
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
        let mut model = model.as_ref().clone();

        let texture = load_glove_texture(asset_cache);

        let emissive = asset_cache
            .get_opt::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, GLOVE_EMISSIVE_TEXTURE)
            .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>);
        // Immutable material sets per colour: drawing the second hand cannot
        // recolour scene objects already submitted for the first hand.
        let authored = model.to_scene_objects();
        let make_materials = |base_emissivity| {
            HandLight::ALL
                .into_iter()
                .map(|light| {
                    authored
                        .iter()
                        .map(|object| match &texture {
                            Some(texture) => Rc::new(RefCell::new(match &emissive {
                                Some(mask) => SkinnedMaterial::create_with_light(
                                    texture.clone(),
                                    base_emissivity,
                                    0.0,
                                    mask.clone(),
                                    light.tint(),
                                ),
                                None => {
                                    SkinnedMaterial::create(texture.clone(), base_emissivity, 0.0)
                                }
                            })),
                            None => object.material.clone(),
                        })
                        .collect()
                })
                .collect()
        };

        // Legacy/menu gloves keep their full-texture glow. With authored
        // lighting only the status-light mask emits; the glove surface is lit.
        let materials = make_materials(1.0);
        let lit_materials = make_materials(0.0);
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        let delta_basis = retarget.delta_basis(&mut model);
        let spheres = asset_cache
            .get_raw_reader(GLOVE_MODEL)
            .and_then(|reader| {
                let mut bytes = Vec::new();
                std::io::copy(&mut *reader.borrow_mut(), &mut bytes).ok()?;
                dark::importers::skinned_vertices_from_glb_bytes(&bytes)
            })
            .map(|vertices| finger_spheres(&mut model, &retarget, &vertices))
            .unwrap_or_else(|| {
                tracing::warn!("glove skin unreadable: Tighten uses the auto-fit's finger radius");
                let open = hand_pose::open_right_hand();
                let frames = skin_frames(&mut model, &retarget, &open, Handedness::Right);
                uniform_spheres(&frames, crate::vr_grip::FINGER_RADIUS)
            });

        Some(Self {
            model,
            retarget,
            open: hand_pose::open_right_hand(),
            fist: hand_pose::fist_right_hand(),
            point: hand_pose::point_right_hand(),
            materials,
            lit_materials,
            wrist_frames: None,
            open_joints: [None, None],
            delta_basis,
            spheres,
        })
    }

    /// The glove's 26 skin joints posed with `pose`, in calibrated hand space
    /// at authored size (world scale 1), as grips are fitted.
    fn authored_joints(
        &mut self,
        pose: &Pose,
        handedness: Handedness,
    ) -> crate::hand_capture::HandJoints {
        skin_joints(&mut self.model, &self.retarget, pose, handedness)
    }

    /// The open glove's joints in calibrated hand space as drawn, to match
    /// against a tracked hand.
    pub fn open_joints(&mut self, handedness: Handedness) -> crate::hand_capture::HandJoints {
        let slot = handedness as usize;
        if self.open_joints[slot].is_none() {
            let open = self.open.clone();
            self.open_joints[slot] = Some(self.authored_joints(&open, handedness));
        }
        let personal = crate::world_scale::WorldScale::active().personal();
        self.open_joints[slot]
            .unwrap()
            .map(|joint| joint * personal)
    }

    /// The open glove at drawn size, to fit a tracked hand with.
    pub(crate) fn rig(&mut self, handedness: Handedness) -> crate::hand_capture::GloveRig {
        crate::hand_capture::GloveRig {
            open: self.open_joints(handedness),
            basis: rig_basis(self.delta_basis, handedness),
        }
    }

    /// Sample the renderer's actual retargeted finger arcs once for fitting.
    pub fn grip_kinematics(&mut self, handedness: Handedness) -> crate::vr_grip::GripKinematics {
        use crate::vr_grip::{CURL_STEPS, GripKinematics};
        let mut fingers: [Vec<Vec<Vector3<f32>>>; 5] = std::array::from_fn(|_| Vec::new());
        let mut open_joints = Vec::new();
        let mut closed_joints = Vec::new();
        for step in 0..=CURL_STEPS {
            let pose = self.open.blend(&self.fist, step as f32 / CURL_STEPS as f32);
            let joints = self.authored_joints(&pose, handedness);
            if step == 0 {
                open_joints = joints.to_vec();
            }
            if step == CURL_STEPS {
                closed_joints = joints.to_vec();
            }
            for (finger, points) in finger_samples(&joints).into_iter().enumerate() {
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

    /// Each finger with the glove posed by `bones`, its phalanges as thick
    /// as the glove's: what `GripSurface::tighten` tests.
    pub fn finger_chains(
        &mut self,
        bones: &crate::hand_pose::FingerBones,
        handedness: Handedness,
    ) -> [crate::vr_grip::FingerChain; 5] {
        let pose = Pose::from_finger_bones(bones);
        let frames = skin_frames(&mut self.model, &self.retarget, &pose, handedness);
        finger_chains(&frames, &self.spheres)
    }

    /// A readable plate basis measured from this glove's wrist and palm, in
    /// calibrated hand space. Keep text right-handed even on the mirrored glove.
    pub(crate) fn wrist_frame(&mut self, hand: Handedness) -> Matrix4<f32> {
        use cgmath::InnerSpace;
        if self.wrist_frames.is_none() {
            self.wrist_frames = Some([Handedness::Left, Handedness::Right].map(|side| {
                let rig = self.grip_kinematics(side);
                self.retarget.apply(&self.open, &mut self.model);
                let node = self.model.skeleton().node_index_for_joint(1).unwrap();
                let wrist = (crate::vr_grip::glove_to_hand(side)
                    * self.model.get_global_transform(node).unwrap())
                .w
                .truncate();
                let normal = -rig.normal;
                let along = rig.palm - wrist;
                let up = (along - normal * along.dot(normal)).normalize();
                let right = up.cross(normal).normalize();
                Matrix4::from_cols(
                    right.extend(0.0),
                    up.extend(0.0),
                    normal.extend(0.0),
                    wrist.extend(1.0),
                )
            }));
        }
        Matrix4::from_scale(crate::world_scale::WorldScale::active().personal())
            * self.wrist_frames.unwrap()[if hand == Handedness::Left { 0 } else { 1 }]
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
        anticipation: Option<&crate::hand_anticipation::HandAnticipation>,
        fitted: Option<(HandShape, f32)>,
        light: HandLight,
        lights: Option<Rc<engine::scene::light::LightArray>>,
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
        let has_fitted = fitted.is_some();
        let fitted_pose = match fitted {
            Some((HandShape::Curls(fitted), blend)) => {
                let t = blend.clamp(0.0, 1.0);
                amounts = FingerAmounts {
                    thumb: amounts.thumb * (1.0 - t) + fitted.thumb * t,
                    index: amounts.index * (1.0 - t) + fitted.index * t,
                    middle: amounts.middle * (1.0 - t) + fitted.middle * t,
                    ring: amounts.ring * (1.0 - t) + fitted.ring * t,
                    pinky: amounts.pinky * (1.0 - t) + fitted.pinky * t,
                };
                None
            }
            // Bone poses blend in rotation space from the curl pose below.
            Some((HandShape::Pose(pose), blend)) => Some((pose, blend)),
            None => None,
        };
        let pose = match anticipation.filter(|_| !holding && !has_fitted) {
            Some(preview) => preview.pose(&self.open, &self.point, &self.fist, amounts),
            None => self.open.blend_per_finger(&self.fist, &amounts),
        };
        let pose = match fitted_pose {
            Some((fitted, blend)) => pose.blend(&fitted, blend.clamp(0.0, 1.0)),
            None => pose,
        };
        let materials = if lights.is_some() {
            &self.lit_materials
        } else {
            &self.materials
        };
        let mut objects = Self::render_posed(
            &mut self.model,
            &self.retarget,
            &materials[light as usize],
            &pose,
            position,
            rotation,
            handedness,
        );
        let tag = Rc::new(engine::scene::SceneObjectDebugTag {
            model: Some(GLOVE_MODEL.to_owned()),
            name: Some(format!("{handedness:?} glove")),
            ..Default::default()
        });
        for object in &mut objects {
            object.set_lights(lights.clone());
            object.set_debug_tag(Some(tag.clone()));
        }
        objects
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
            &self.materials[HandLight::Off as usize],
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
            * crate::vr_grip::glove_to_hand(handedness)
            * Matrix4::from_scale(crate::world_scale::WorldScale::active().personal());

        let mut objects = model.to_scene_objects_with_skinning();
        for (object, material) in objects.iter_mut().zip(materials) {
            object.replace_material(material.clone());
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

/// Each finger's phalanges Tighten tests, thumb first, by the joint at each
/// one's base (its tip is the next joint): the thumb's from its metacarpal,
/// which presses a grip as much as its pad does (`vr_grip::FingerChain`).
const PHALANX_JOINTS: [std::ops::Range<usize>; 5] = [2..5, 7..10, 12..15, 17..20, 22..25];

/// The glove's skin vertices posed with `pose`, in calibrated hand space at
/// authored size, each with the joint it is weighted most to.
fn posed_skin(
    model: &mut GlbModel,
    retarget: &HandPoseRetarget,
    vertices: &[engine::scene::VertexPositionTextureSkinnedNormal],
    pose: &Pose,
    handedness: Handedness,
) -> Vec<(Vector3<f32>, usize)> {
    retarget.apply(pose, model);
    let skinning = model.get_skinning_matrices();
    let to_hand = crate::vr_grip::glove_to_hand(handedness);
    use cgmath::{EuclideanSpace, Transform};
    vertices
        .iter()
        .filter_map(|vertex| {
            let skinned = dark::importers::skinned_point(vertex, &skinning)?;
            let heaviest = (0..4)
                .max_by(|&a, &b| vertex.bone_weights[a].total_cmp(&vertex.bone_weights[b]))
                .unwrap();
            Some((
                to_hand.transform_point(skinned).to_vec(),
                vertex.bone_indices[heaviest] as usize,
            ))
        })
        .collect()
}

/// Per finger (thumb first), each phalanx's spheres (`FingerChain`) in the
/// frame of the joint at its base, so they pose with it.
type FingerSpheres = [Vec<[Sphere; PHALANX_SAMPLES]>; 5];

/// Each finger's phalanges, its `spheres` posed by the joint `frames`.
fn finger_chains(
    frames: &[Matrix4<f32>; crate::hand_capture::HAND_JOINT_COUNT],
    spheres: &FingerSpheres,
) -> [crate::vr_grip::FingerChain; 5] {
    use cgmath::{EuclideanSpace, Transform};
    std::array::from_fn(|finger| crate::vr_grip::FingerChain {
        phalanges: spheres[finger]
            .iter()
            .zip(PHALANX_JOINTS[finger].clone())
            .map(|(spheres, joint)| {
                spheres.map(|sphere| Sphere {
                    center: frames[joint]
                        .transform_point(cgmath::Point3::from_vec(sphere.center))
                        .to_vec(),
                    radius: sphere.radius,
                })
            })
            .collect(),
    })
}

/// `centers` (calibrated hand space, posed by `frames`) back in the frame
/// of each phalanx's base joint.
fn phalanx_local(
    frames: &[Matrix4<f32>; crate::hand_capture::HAND_JOINT_COUNT],
    joint: usize,
    spheres: [Sphere; PHALANX_SAMPLES],
) -> [Sphere; PHALANX_SAMPLES] {
    use cgmath::{EuclideanSpace, SquareMatrix, Transform};
    let inverse = frames[joint].invert().expect("joint frames are invertible");
    spheres.map(|sphere| Sphere {
        center: inverse
            .transform_point(cgmath::Point3::from_vec(sphere.center))
            .to_vec(),
        radius: sphere.radius,
    })
}

/// Spheres `radius` thick along each phalanx's axis, posed by `frames`.
fn uniform_spheres(
    frames: &[Matrix4<f32>; crate::hand_capture::HAND_JOINT_COUNT],
    radius: f32,
) -> FingerSpheres {
    PHALANX_JOINTS.map(|range| {
        range
            .map(|joint| {
                let (a, b) = (frames[joint].w.truncate(), frames[joint + 1].w.truncate());
                let along = crate::vr_grip::FingerChain::uniform(&[a, b], radius).phalanges[0];
                phalanx_local(frames, joint, along)
            })
            .collect()
    })
}

/// How the glove's fingers fill out, measured from its skin. At each of a
/// phalanx's samples, the finger's cross-section toward its pad and its
/// back (the median distance from the axis to the skin near there, each
/// way) gives a sphere spanning the two; at the fingertip, a sphere about
/// the tip joint out to the skin past it. The bones run nearer the back of
/// a finger than its pad, and the tip joint sits near the end of the
/// fingertip.
fn finger_spheres(
    model: &mut GlbModel,
    retarget: &HandPoseRetarget,
    vertices: &[engine::scene::VertexPositionTextureSkinnedNormal],
) -> FingerSpheres {
    use cgmath::InnerSpace;
    let step = 1.0 / (PHALANX_SAMPLES - 1) as f32;
    let hand = Handedness::Right;
    let open = hand_pose::open_right_hand();
    let curling = open.blend(&hand_pose::fist_right_hand(), 0.1);
    let frames = skin_frames(model, retarget, &open, hand);
    let joints = frames.map(|frame| frame.w.truncate());
    let curled = skin_joints(model, retarget, &curling, hand);
    let skin = posed_skin(model, retarget, vertices, &open, hand);
    let median = |mut distances: Vec<f32>| {
        distances.sort_by(f32::total_cmp);
        distances.get(distances.len() / 2).copied()
    };
    PHALANX_JOINTS.map(|range| {
        // Per phalanx: its base, its axis and the way its tip curls.
        let phalanges: Vec<_> = range
            .clone()
            .map(|joint| {
                let (base, axis) = (joints[joint], joints[joint + 1] - joints[joint]);
                let moved = curled[joint + 1] - joints[joint + 1];
                let curl = moved - axis * (moved.dot(axis) / axis.magnitude2());
                // A phalanx that does not curl has no pad side: measured all round.
                let pad = match curl.magnitude() > 1e-6 {
                    true => curl.normalize(),
                    false => Vector3::new(0.0, 0.0, 0.0),
                };
                (base, axis, pad)
            })
            .collect();
        let tip = phalanges.len() - 1;
        // Skin distances near each sample, toward the pad and the back.
        let mut near =
            vec![[(); PHALANX_SAMPLES].map(|_| (Vec::new(), Vec::new())); phalanges.len()];
        let mut cap = Vec::new();
        for (p, _) in skin
            .iter()
            .filter(|(_, weighted)| (range.start..=range.end).contains(weighted))
        {
            let (k, t, off) = phalanges
                .iter()
                .enumerate()
                .map(|(k, (base, axis, _))| {
                    let t = (p - base).dot(*axis) / axis.magnitude2();
                    (k, t, p - (base + axis * t.clamp(0.0, 1.0)))
                })
                .min_by(|a, b| a.2.magnitude2().total_cmp(&b.2.magnitude2()))
                .unwrap();
            let sample = (t / step).round().clamp(0.0, (PHALANX_SAMPLES - 1) as f32) as usize;
            // No pad side (a phalanx that does not curl): all of it counts as pad.
            let pad = phalanges[k].2;
            let facing = match pad.magnitude2() > 0.0 {
                true => off.dot(pad) / off.magnitude(),
                false => 1.0,
            };
            if k == tip && t >= 1.0 {
                cap.push(off.magnitude());
            } else if !(k == tip && sample == PHALANX_SAMPLES - 1)
                && (t - sample as f32 * step).abs() <= step * 0.5
            {
                match facing {
                    f if f > 0.5 => near[k][sample].0.push(off.magnitude()),
                    f if f < -0.5 => near[k][sample].1.push(off.magnitude()),
                    _ => {}
                }
            }
        }
        let cap = median(cap).unwrap_or(crate::vr_grip::FINGER_RADIUS);
        phalanges
            .iter()
            .zip(near)
            .enumerate()
            .map(|(k, ((base, axis, pad), near))| {
                let reach = near.map(|(pad, back)| (median(pad), median(back)));
                // A sample with no skin near it (a knuckle, carried by the
                // metacarpal) takes its nearest neighbour's.
                let nearest = |i: usize, side: fn(&(Option<f32>, Option<f32>)) -> Option<f32>| {
                    (0..PHALANX_SAMPLES)
                        .filter_map(|other| side(&reach[other]).map(|r| (i.abs_diff(other), r)))
                        .min_by_key(|(distance, _)| *distance)
                        .map(|(_, r)| r)
                };
                let length = axis.magnitude();
                let spheres = std::array::from_fn(|i| {
                    let t = i as f32 * step;
                    if k == tip && i == PHALANX_SAMPLES - 1 {
                        return Sphere {
                            center: base + axis,
                            radius: cap,
                        };
                    }
                    let toward = nearest(i, |r| r.0).unwrap_or(crate::vr_grip::FINGER_RADIUS);
                    let away = nearest(i, |r| r.1).unwrap_or(toward);
                    let radius = (toward + away) * 0.5;
                    // Nothing reaches past the fingertip.
                    let radius = match k == tip {
                        true => radius.min((1.0 - t) * length + cap),
                        false => radius,
                    };
                    Sphere {
                        center: base + axis * t + pad * ((toward - away) * 0.5),
                        radius,
                    }
                });
                phalanx_local(&frames, range.start + k, spheres)
            })
            .collect()
    })
}

/// Points along each finger's joints (half a phalanx apart, thumb first):
/// the samples grip fitting tests against an item.
fn finger_samples(joints: &crate::hand_capture::HandJoints) -> [Vec<Vector3<f32>>; 5] {
    [3..5, 7..10, 12..15, 17..20, 22..25].map(|range| {
        range
            .flat_map(|joint| {
                [0.0, 0.5, 1.0].map(|t| joints[joint] * (1.0 - t) + joints[joint + 1] * t)
            })
            .collect()
    })
}

/// The glove model's 26 skin joints posed with `pose`, in calibrated hand
/// space at authored size.
fn skin_joints(
    model: &mut GlbModel,
    retarget: &HandPoseRetarget,
    pose: &Pose,
    handedness: Handedness,
) -> crate::hand_capture::HandJoints {
    skin_frames(model, retarget, pose, handedness).map(|frame| frame.w.truncate())
}

/// The glove model's skin joint frames posed with `pose`, in calibrated
/// hand space at authored size.
fn skin_frames(
    model: &mut GlbModel,
    retarget: &HandPoseRetarget,
    pose: &Pose,
    handedness: Handedness,
) -> [Matrix4<f32>; crate::hand_capture::HAND_JOINT_COUNT] {
    let transform = crate::vr_grip::glove_to_hand(handedness);
    retarget.apply(pose, model);
    std::array::from_fn(|joint| {
        let node = model.skeleton().node_index_for_joint(joint).unwrap();
        transform * model.get_global_transform(node).unwrap()
    })
}

/// A rig's `basis` for the glove drawn as `handedness`: the model's delta
/// basis carried into calibrated hand space, mirror included.
fn rig_basis(delta_basis: cgmath::Matrix3<f32>, handedness: Handedness) -> cgmath::Matrix3<f32> {
    let m = crate::vr_grip::glove_to_hand(handedness) / GLOVE_SCALE;
    cgmath::Matrix3::from_cols(m.x.truncate(), m.y.truncate(), m.z.truncate()) * delta_basis
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hand_capture::{GloveRig, fit_hand};

    /// The glove model and its retarget, without an asset cache.
    fn glove() -> (GlbModel, HandPoseRetarget) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/vr_glove_model.glb");
        let bytes = std::fs::read(path).expect("read vr_glove_model.glb");
        let skeleton = dark::importers::skeleton_from_glb_bytes(&bytes).unwrap();
        let origin = cgmath::Point3::new(0.0, 0.0, 0.0);
        let model = GlbModel::new(Vec::new(), collision::Aabb3::new(origin, origin), skeleton);
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        (model, retarget)
    }

    /// `GloveRenderer::rig` at authored size, without an asset cache.
    fn rig(model: &mut GlbModel, retarget: &HandPoseRetarget, hand: Handedness) -> GloveRig {
        GloveRig {
            open: skin_joints(model, retarget, &hand_pose::open_right_hand(), hand),
            basis: rig_basis(retarget.delta_basis(model), hand),
        }
    }

    /// Hands fitted with the real glove: the pose data's grips at about the
    /// glove's size (real hands, in proportions a little unlike the glove's)
    /// reach every tip a finger of the glove's length can, and the joints the
    /// fit reports are the ones the glove draws with its bones.
    #[test]
    fn the_glove_reaches_real_proportioned_hands_and_draws_what_it_fit() {
        use crate::hand_capture::tests::{assert_reached, assert_within_limits, grips, tracked};
        use cgmath::{InnerSpace, Rotation};
        let (mut model, retarget) = glove();
        let reached = 0.003 / crate::METERS_PER_WORLD_UNIT;
        for hand in [Handedness::Right, Handedness::Left] {
            let rig = rig(&mut model, &retarget, hand);
            // The pose data's hand is larger than the glove as drawn.
            let open = tracked(&hand_pose::open_right_hand(), hand)
                .map(|p| p / crate::METERS_PER_WORLD_UNIT);
            let size = fit_hand(&open, &rig).unwrap().size_ratio;
            // Its fist's little finger curls tighter than the glove's can.
            for (name, pose) in grips().into_iter().filter(|(name, _)| *name != "fist") {
                for factor in [0.95, 1.0, 1.05] {
                    let joints = tracked(&pose, hand).map(|p| p / crate::METERS_PER_WORLD_UNIT);
                    let joints = joints.map(|p| joints[1] + (p - joints[1]) * (factor / size));
                    let fit = fit_hand(&joints, &rig).unwrap();
                    assert_within_limits(&fit);
                    let name = format!("{hand:?} {name} x{factor}");
                    assert_reached(&fit, &joints, &rig.open, reached, &name);
                    let pose = Pose::from_finger_bones(&fit.bones);
                    let drawn = skin_joints(&mut model, &retarget, &pose, hand)
                        .map(|p| fit.position + fit.rotation.rotate_vector(p));
                    for (joint, (a, b)) in drawn.iter().zip(&fit.joints).enumerate() {
                        let off = (a - b).magnitude() * crate::METERS_PER_WORLD_UNIT * 1000.0;
                        assert!(off < 0.1, "{name} joint {joint}: {off} mm");
                    }
                }
            }
        }
    }

    /// The hands the grip pose tool's e2e test tracks (flat, built ones):
    /// thumb to middle land on them; ring and little are longer than the
    /// glove's and come as near as straight fingers can.
    #[test]
    fn the_glove_reaches_the_built_hands() {
        let (mut model, retarget) = glove();
        let reached = 0.003 / crate::METERS_PER_WORLD_UNIT;
        for hand in [Handedness::Right, Handedness::Left] {
            let rig = rig(&mut model, &retarget, hand);
            for (name, joints) in crate::hand_capture::tests::foreign_hands() {
                let joints = joints.map(|p| hand.mirror_point(p) / crate::METERS_PER_WORLD_UNIT);
                let fit = fit_hand(&joints, &rig).unwrap();
                crate::hand_capture::tests::assert_reached(&fit, &joints, &rig.open, reached, name);
                let errors = &fit.tip_errors[..3];
                assert!(
                    errors.iter().all(|e| *e < reached),
                    "{hand:?} {name}: {errors:?}"
                );
            }
        }
    }

    /// The glove's skin vertices, read as the game reads its model.
    fn skin_vertices() -> Vec<engine::scene::VertexPositionTextureSkinnedNormal> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/vr_glove_model.glb");
        let bytes = std::fs::read(path).expect("read vr_glove_model.glb");
        dark::importers::skinned_vertices_from_glb_bytes(&bytes).unwrap()
    }

    fn mm(world: f32) -> f32 {
        world * crate::METERS_PER_WORLD_UNIT * 1000.0
    }

    /// Each chain's first phalanx turns with the bone Tighten moves for it
    /// (`FingerBones` index + the first posed joint).
    #[test]
    fn phalanx_joints_match_the_bones_tighten_moves() {
        for (joints, bone) in PHALANX_JOINTS.iter().zip(crate::vr_grip::PHALANX_BONES) {
            assert_eq!(joints.start, bone + hand_pose::FIRST_POSED_JOINT);
        }
    }

    /// The glove's fingers are thicker than the auto-fit's 7 mm and sit
    /// toward their pads off the bones; the tips, past the tip joints, are
    /// thinner.
    #[test]
    fn finger_spheres_are_thick_toward_the_pad_and_thin_at_the_tip() {
        use cgmath::InnerSpace;
        let (mut model, retarget) = glove();
        let spheres = finger_spheres(&mut model, &retarget, &skin_vertices());
        let open = hand_pose::open_right_hand();
        let frames = skin_frames(&mut model, &retarget, &open, Handedness::Right);
        for (finger, chain) in finger_chains(&frames, &spheres).iter().enumerate() {
            let joints = PHALANX_JOINTS[finger]
                .clone()
                .map(|j| (frames[j].w, frames[j + 1].w));
            // Per sphere: its radius, and how far its centre is off the bone.
            let shape: Vec<Vec<(f32, f32)>> = chain
                .phalanges
                .iter()
                .zip(joints)
                .map(|(spheres, (a, b))| {
                    let (a, axis) = (a.truncate(), (b - a).truncate().normalize());
                    spheres
                        .iter()
                        .map(|s| {
                            let off = s.center - a;
                            (mm(s.radius), mm((off - axis * off.dot(axis)).magnitude()))
                        })
                        .collect()
                })
                .collect();
            println!("finger {finger} (radius, off the bone) mm: {shape:.1?}");
            let (tip, along) = shape
                .concat()
                .split_last()
                .map(|(t, a)| (*t, a.to_vec()))
                .unwrap();
            assert!(
                along
                    .iter()
                    .all(|(r, off)| (6.0..25.0).contains(r) && *off < 6.0),
                "finger {finger}: {shape:?}"
            );
            assert!((1.0..9.0).contains(&tip.0), "finger {finger}: {shape:?}");
        }
    }

    /// Tightened against the glove's measured thickness, the drawn glove's
    /// skin comes to rest `TIGHTEN_CLEARANCE` (1 mm) off the item, give or
    /// take 2 mm: a block the open fingers curl down onto, a bottle under
    /// them and a ledge whose edge they wrap. The auto-fit's flat 7 mm misses
    /// by up to 6.6 mm.
    #[test]
    fn tightened_fingers_skin_rests_at_the_clearance() {
        use crate::vr_grip::{FingerFit, FingerPose, GripSurface, ResolvedGrip};
        use cgmath::{EuclideanSpace, InnerSpace, One};
        let hand = Handedness::Right;
        let (mut model, retarget) = glove();
        let vertices = skin_vertices();
        let spheres = finger_spheres(&mut model, &retarget, &vertices);
        let flat = uniform_spheres(
            &skin_frames(&mut model, &retarget, &hand_pose::open_right_hand(), hand),
            crate::vr_grip::FINGER_RADIUS,
        );
        let metres = |m: f32| m / crate::METERS_PER_WORLD_UNIT;
        // The palm's frame, as `grip_kinematics` takes it.
        let open = hand_pose::open_right_hand();
        let joints = skin_joints(&mut model, &retarget, &open, hand);
        let fist = skin_joints(&mut model, &retarget, &hand_pose::fist_right_hand(), hand);
        let knuckles = (joints[7] + joints[12] + joints[17] + joints[22]) * 0.25;
        let along = (knuckles - joints[1]).normalize();
        let mut normal = along.cross(joints[7] - joints[22]).normalize();
        if normal.dot(fist[15] - knuckles) < 0.0 {
            normal = -normal;
        }
        let across = normal.cross(along);
        // How far below the knuckles the open fingers' skin reaches, out to
        // `reach` along them.
        let fingers = PHALANX_JOINTS[1].start..=PHALANX_JOINTS[4].end;
        let open_skin = posed_skin(&mut model, &retarget, &vertices, &open, hand);
        let lowest = |reach: f32| {
            open_skin
                .iter()
                .filter(|(p, joint)| fingers.contains(joint) && (p - knuckles).dot(along) < reach)
                .map(|(p, _)| (p - knuckles).dot(normal))
                .fold(f32::NEG_INFINITY, f32::max)
        };
        // `vertices` (x along, y across, z the way the fingers curl) at `at`.
        let placed = |(corners, indices): (Vec<rapier3d::na::Point3<f32>>, Vec<[u32; 3]>),
                      at: Vector3<f32>| {
            indices
                .iter()
                .map(|t| {
                    t.map(|i| {
                        let p = corners[i as usize];
                        cgmath::Point3::from_vec(at + along * p.x + across * p.y + normal * p.z)
                    })
                })
                .collect::<Vec<_>>()
        };
        // A block 5 mm under the open fingers, its top `depth` down.
        let depth = lowest(f32::INFINITY) + metres(0.005);
        let half = rapier3d::na::Vector3::new(metres(0.1), metres(0.1), metres(0.05));
        let block = placed(
            rapier3d::prelude::Cuboid::new(half).to_trimesh(),
            knuckles + along * metres(0.1) + normal * (depth + metres(0.05)),
        );
        let off_block = |p: Vector3<f32>| depth - (p - knuckles).dot(normal);
        // A 6 cm bottle across the fingers' bases, 2 mm under them.
        let radius = metres(0.03);
        let axis = knuckles
            + along * metres(0.025)
            + normal * (lowest(metres(0.06)) + metres(0.002) + radius);
        let rod = placed(
            rapier3d::prelude::Cylinder::new(metres(0.1), radius).to_trimesh(96),
            axis,
        );
        let off_rod = |p: Vector3<f32>| {
            let v = p - axis;
            (v - across * v.dot(across)).magnitude() - radius
        };
        // A box `half` (along, across, the way the fingers curl) about
        // `center`, and how far a point is off it.
        let boxed = |center: Vector3<f32>, half: Vector3<f32>| {
            let triangles = placed(
                rapier3d::prelude::Cuboid::new(rapier3d::na::Vector3::new(half.x, half.y, half.z))
                    .to_trimesh(),
                center,
            );
            let off = move |p: Vector3<f32>| {
                let v = p - center;
                let q = Vector3::new(v.dot(along).abs(), v.dot(across).abs(), v.dot(normal).abs())
                    - half;
                let outside = Vector3::new(q.x.max(0.0), q.y.max(0.0), q.z.max(0.0)).magnitude();
                outside + q.x.max(q.y).max(q.z).min(0.0)
            };
            (triangles, off)
        };
        // A ledge under the fingers' bases, 2 mm down, its edge 4 cm out:
        // the bases land on it and the rest of each finger wraps its edge.
        let top = lowest(metres(0.04)) + metres(0.002);
        let half = Vector3::new(metres(0.04), metres(0.1), metres(0.05));
        let (ledge, off_ledge) = boxed(knuckles + normal * (top + half.z), half);
        // A 2.4 cm knob where the thumb's tip swings to half way to a fist.
        let mut halfway = hand_pose::FingerAmounts::default();
        halfway.thumb = 0.5;
        let thumb_tip = skin_joints(
            &mut model,
            &retarget,
            &open.blend_per_finger(&hand_pose::fist_right_hand(), &halfway),
            hand,
        )[5];
        let (knob, off_knob) = boxed(
            thumb_tip,
            Vector3::new(metres(0.012), metres(0.012), metres(0.012)),
        );
        let grip = ResolvedGrip {
            item_scale: 1.0,
            pose_family: "broad".to_owned(),
            offset: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::one(),
            hand_rotation: Quaternion::one(),
            fingers: FingerPose::curls([0.0; 5]),
            contacts: [None; 5],
            anchor: [0.0; 3],
            score: 0.0,
        };
        // Per finger that touched (thumb..little), how far its skin rests
        // off `triangles` by `off`, tightened with `spheres`.
        let mut skin_gaps_mm = |triangles: &[[cgmath::Point3<f32>; 3]],
                                off: &dyn Fn(Vector3<f32>) -> f32,
                                spheres: &FingerSpheres| {
            let surface = GripSurface::new(triangles).unwrap();
            let out = surface.tighten(&grip, false, [true; 5], |bones| {
                let pose = Pose::from_finger_bones(bones);
                finger_chains(&skin_frames(&mut model, &retarget, &pose, hand), spheres)
            });
            let pose = Pose::from_finger_bones(&out.grip.fingers.rest_bones());
            let skin = posed_skin(&mut model, &retarget, &vertices, &pose, hand);
            std::array::from_fn::<_, 5, _>(|f| {
                let joints = PHALANX_JOINTS[f].start..=PHALANX_JOINTS[f].end;
                matches!(out.fingers[f], FingerFit::Touching { .. }).then(|| {
                    skin.iter()
                        .filter(|(_, joint)| joints.contains(joint))
                        .map(|(p, _)| mm(off(*p)))
                        .fold(f32::INFINITY, f32::min)
                })
            })
        };
        // A capsule only approximates the skin: each finger within 2 mm of
        // the clearance, on average within half a millimetre.
        let within =
            |gaps: &[Option<f32>; 5]| gaps.iter().flatten().all(|g| (-1.0..3.0).contains(g));
        let results = [
            ("block", &block, &off_block as &dyn Fn(Vector3<f32>) -> f32),
            ("bottle", &rod, &off_rod),
            ("ledge", &ledge, &off_ledge),
            ("knob", &knob, &off_knob),
        ]
        .map(|(name, triangles, off)| {
            let measured = skin_gaps_mm(triangles, off, &spheres);
            let flat = skin_gaps_mm(triangles, off, &flat);
            println!("{name}: skin off it (mm), thumb..little: measured {measured:.1?}, flat 7 mm {flat:.1?}");
            (name, measured, flat)
        });
        for (name, measured, _) in &results {
            // Index..ring meet the three under the fingers; the thumb the knob.
            let meet = if *name == "knob" { 0..1 } else { 1..4 };
            assert!(
                measured[meet].iter().all(Option::is_some),
                "{name}: {measured:?}"
            );
            assert!(within(measured), "{name}: {measured:?}");
        }
        let gaps: Vec<f32> = results
            .iter()
            .flat_map(|(_, measured, _)| measured.iter().flatten().copied())
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        assert!((0.5..1.5).contains(&mean), "{mean} mm on average");
        assert!(
            results.iter().any(|(_, _, flat)| !within(flat)),
            "the measured radii are what land the skin"
        );
    }

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
