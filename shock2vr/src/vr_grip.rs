//! Deterministic pickup fitting against visible triangles. All outputs are
//! hand-local; tracked motion never re-runs the search or changes item scale.
use cgmath::{
    EuclideanSpace, InnerSpace, Matrix4, One, Point3, Quaternion, Rotation, Rotation3, Vector3,
    vec3,
};
use rapier3d::{
    na,
    parry::{
        query::{self, PointQuery, Ray, RayCast},
        shape::Segment,
    },
    prelude::{TriMesh, TriMeshFlags},
};
use serde::{Deserialize, Serialize};

use crate::{
    hand_pose::{
        FINGER_BONE_COUNT, FingerAmounts, FingerBones, HandShape, Pose, fist_right_hand,
        open_right_hand,
    },
    vr_config::Handedness,
    vr_support::GripPose,
};

pub const CURL_STEPS: usize = 24;
/// Bump when solver policy changes require existing results to be rebaked.
pub const SOLVER_REVISION: u32 = 3;
/// The auto-fit's finger thickness; Tighten uses the glove's own
/// (`FingerChain` spheres).
pub const FINGER_RADIUS: f32 = 0.007 / crate::METERS_PER_WORLD_UNIT;

/// Negative authored thumb curl extends beyond the open pose. Auto-fit still
/// searches the measured open-to-fist arcs; other fingers remain in 0..=1.
pub const MIN_THUMB_CURL: f32 = -1.0;

pub fn valid_finger_curls(curls: &[f32; 5]) -> bool {
    curls.iter().enumerate().all(|(finger, value)| {
        let min = if finger == 0 { MIN_THUMB_CURL } else { 0.0 };
        value.is_finite() && (min..=1.0).contains(value)
    })
}

/// Sampled from the very same glove skeleton and pose endpoints used to draw it.
pub struct GripKinematics {
    pub fingers: [Vec<Vec<Vector3<f32>>>; 5],
    pub palm: Vector3<f32>,
    pub normal: Vector3<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedGrip {
    /// Uniform held-item scale; the glove calibration is unchanged.
    #[serde(default = "default_item_scale")]
    pub item_scale: f32,
    pub pose_family: String,
    /// Item origin and rotation in the glove's frame (world units): how the
    /// hand sits on the item.
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    /// The glove's turn off the tracked controller, about the hand origin:
    /// a gun's aim correction (`weapon_muzzle::aim_turn`). Play puts the
    /// glove at controller · this and the item at controller · this ·
    /// (`offset`, `rotation`).
    #[serde(default = "Quaternion::one", skip_serializing_if = "One::is_one")]
    pub hand_rotation: Quaternion<f32>,
    #[serde(flatten)]
    pub fingers: FingerPose,
    /// Contact samples in item-local space; null means no contact was found.
    pub contacts: [Option<[f32; 3]>; 5],
    pub anchor: [f32; 3],
    pub score: f32,
}

fn default_item_scale() -> f32 {
    1.0
}

impl ResolvedGrip {
    /// The glove a controller at `controller` draws: turned by `hand_rotation`
    /// about the hand origin. An untracked (zero) rotation stays untracked.
    pub fn glove_pose(&self, controller: GripPose) -> GripPose {
        GripPose {
            position: controller.position,
            rotation: controller.rotation * self.hand_rotation,
        }
    }

    /// `glove_pose` inverted: the controller that draws the glove at `glove`.
    pub fn controller_pose(&self, glove: GripPose) -> GripPose {
        GripPose {
            position: glove.position,
            rotation: glove.rotation * self.hand_rotation.conjugate(),
        }
    }

    /// Keep authored grip files in baseline units; scale only the runtime copy.
    pub(crate) fn for_world_scale(self) -> Self {
        self.rescaled(crate::world_scale::WorldScale::active().personal())
    }

    /// Scale the hand-space size and placement; item-local data is unchanged.
    pub(crate) fn rescaled(mut self, personal: f32) -> Self {
        self.item_scale *= personal;
        self.offset *= personal;
        self
    }
    /// Convert a grip fitted against scaled triangles back to source-mesh
    /// coordinates. The hand-space offset already has the correct world size.
    pub fn with_item_scale(mut self, scale: f32) -> Self {
        self.item_scale = scale;
        self.anchor = self.anchor.map(|value| value / scale);
        self.contacts = self
            .contacts
            .map(|contact| contact.map(|point| point.map(|value| value / scale)));
        self
    }

    pub fn is_valid(&self) -> bool {
        [
            self.offset.x,
            self.offset.y,
            self.offset.z,
            self.rotation.s,
            self.rotation.v.x,
            self.rotation.v.y,
            self.rotation.v.z,
            self.score,
        ]
        .into_iter()
        .all(f32::is_finite)
            && self.item_scale.is_finite()
            && self.item_scale > 0.0
            && self.item_scale <= 10.0
            && is_unit_quaternion(self.rotation)
            && is_unit_quaternion(self.hand_rotation)
            && self.fingers.is_valid()
            && self
                .anchor
                .iter()
                .chain(self.contacts.iter().flatten().flatten())
                .all(|c| c.is_finite())
    }

    /// Bounds of the visible fitted item in the glove's frame, for capture framing.
    pub fn item_bounds(&self, triangles: &[[Point3<f32>; 3]]) -> Option<[[f32; 3]; 2]> {
        use collision::{Aabb, Aabb3};
        let points = triangles.iter().flatten().map(|p| {
            Point3::from_vec(
                self.offset + self.rotation.rotate_vector(p.to_vec() * self.item_scale),
            )
        });
        let bounds = points.fold(None, |bounds: Option<Aabb3<f32>>, p| {
            Some(bounds.map_or_else(|| Aabb3::new(p, p), |b| b.grow(p)))
        })?;
        Some([
            [bounds.min.x, bounds.min.y, bounds.min.z],
            [bounds.max.x, bounds.max.y, bounds.max.z],
        ])
    }

    /// This grip for the other hand: hand and item reflected together through
    /// the hand's mirror plane (`Handedness::mirror`, as the glove is drawn),
    /// the item in the other hand's frame. `mirror` takes this hand's item
    /// frame to the other's (`hand_model_mirror`). With grip `G` and hand
    /// reflection `X`, the other hand's is `X·G·mirror⁻¹`: a rotation, since
    /// both reflections flip handedness. The glove turn is reflected alike,
    /// `X·C·X⁻¹`. Fingers are kept; the glove mirrors them. `None` if
    /// `mirror` is no reflection.
    pub fn mirrored(&self, mirror: Matrix4<f32>) -> Option<ResolvedGrip> {
        use cgmath::{Matrix3, SquareMatrix, Transform};
        let hand = Handedness::Left.mirror();
        let back = mirror.invert()?;
        let linear =
            |m: Matrix4<f32>| Matrix3::from_cols(m.x.truncate(), m.y.truncate(), m.z.truncate());
        // A rotation from a matrix that should be one, if it is.
        let proper = |turned: Matrix3<f32>| {
            let rotation = Quaternion::from(turned).normalize();
            (0..3)
                .all(|c| (Matrix3::from(rotation)[c] - turned[c]).magnitude() < 1e-4)
                .then_some(rotation)
        };
        let rotation = proper(linear(hand) * Matrix3::from(self.rotation) * linear(back))?;
        let hand_rotation =
            proper(linear(hand) * Matrix3::from(self.hand_rotation) * linear(hand.invert()?))?;
        let item = |p: [f32; 3]| mirror.transform_point(Point3::from(p)).into();
        let grip = ResolvedGrip {
            // The item origin moves by the reflection's own translation.
            offset: hand.transform_vector(
                self.offset
                    + self
                        .rotation
                        .rotate_vector(back.w.truncate() * self.item_scale),
            ),
            rotation,
            hand_rotation,
            contacts: self.contacts.map(|contact| contact.map(item)),
            anchor: item(self.anchor),
            ..self.clone()
        };
        grip.is_valid().then_some(grip)
    }
}

/// `model_mirror` (an item's right-hand frame into its left's) as the map
/// from the other hand's frame into `hand`'s.
pub fn hand_model_mirror(model_mirror: Matrix4<f32>, hand: Handedness) -> Option<Matrix4<f32>> {
    use cgmath::SquareMatrix;
    match hand {
        Handedness::Left => Some(model_mirror),
        Handedness::Right => model_mirror.invert(),
    }
}

/// A grip's finger shape: either per-finger curls (fitted or slider-authored)
/// or explicit bone rotations (captured from tracked hands). Untagged, so
/// curl grips keep their original `curls`/`trigger_curls` JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, try_from = "RawFingerPose")]
pub enum FingerPose {
    Curls {
        curls: [f32; 5],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger_curls: Option<[f32; 5]>,
    },
    Bones {
        bones: FingerBones,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger_bones: Option<FingerBones>,
    },
}

/// Decodes every finger field so a grip mixing curls and bones is an error
/// rather than one variant silently dropping the other's data.
#[derive(Deserialize)]
struct RawFingerPose {
    curls: Option<[f32; 5]>,
    trigger_curls: Option<[f32; 5]>,
    bones: Option<FingerBones>,
    trigger_bones: Option<FingerBones>,
}

impl TryFrom<RawFingerPose> for FingerPose {
    type Error = &'static str;

    fn try_from(raw: RawFingerPose) -> Result<Self, Self::Error> {
        match raw {
            RawFingerPose {
                curls: Some(curls),
                trigger_curls,
                bones: None,
                trigger_bones: None,
            } => Ok(FingerPose::Curls {
                curls,
                trigger_curls,
            }),
            RawFingerPose {
                curls: None,
                trigger_curls: None,
                bones: Some(bones),
                trigger_bones,
            } => Ok(FingerPose::Bones {
                bones,
                trigger_bones,
            }),
            _ => Err("a grip needs either curls or bones (with their trigger_ pose), not both"),
        }
    }
}

impl FingerPose {
    pub fn curls(curls: [f32; 5]) -> Self {
        FingerPose::Curls {
            curls,
            trigger_curls: None,
        }
    }

    /// Rest curls; `None` for a bone pose.
    pub fn rest_curls(&self) -> Option<&[f32; 5]> {
        match self {
            FingerPose::Curls { curls, .. } => Some(curls),
            FingerPose::Bones { .. } => None,
        }
    }

    pub fn rest_curls_mut(&mut self) -> Option<&mut [f32; 5]> {
        match self {
            FingerPose::Curls { curls, .. } => Some(curls),
            FingerPose::Bones { .. } => None,
        }
    }

    pub fn curls_at(&self, trigger: f32) -> Option<[f32; 5]> {
        match self {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => Some(blended_curls(*curls, *trigger_curls, trigger)),
            FingerPose::Bones { .. } => None,
        }
    }

    /// This pose frozen at `trigger`, with no separate pressed pose.
    pub fn at(&self, trigger: f32) -> FingerPose {
        match self {
            FingerPose::Curls { .. } => FingerPose::curls(self.curls_at(trigger).unwrap()),
            FingerPose::Bones {
                bones,
                trigger_bones,
            } => FingerPose::Bones {
                bones: blended_bones(bones, trigger_bones.as_ref(), trigger),
                trigger_bones: None,
            },
        }
    }

    pub fn hand_shape_at(&self, trigger: f32) -> HandShape {
        match self.at(trigger) {
            FingerPose::Curls { curls, .. } => HandShape::Curls(finger_amounts(curls)),
            FingerPose::Bones { bones, .. } => HandShape::Pose(Pose::from_finger_bones(&bones)),
        }
    }

    /// The same shape as explicit bones, e.g. as a starting point for a
    /// captured pose. Exact at rest and fully pressed.
    pub fn to_bones(&self) -> FingerPose {
        match self {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => FingerPose::Bones {
                bones: curls_to_bones(*curls),
                trigger_bones: trigger_curls.map(curls_to_bones),
            },
            FingerPose::Bones { .. } => self.clone(),
        }
    }

    /// Curls estimated from a bone pose (`bones_to_curls`); curls are unchanged.
    pub fn to_curls(&self) -> FingerPose {
        match self {
            FingerPose::Curls { .. } => self.clone(),
            FingerPose::Bones {
                bones,
                trigger_bones,
            } => FingerPose::Curls {
                curls: bones_to_curls(bones),
                trigger_curls: trigger_bones.as_ref().map(bones_to_curls),
            },
        }
    }

    /// The rest pose as bones (curls converted exactly).
    pub fn rest_bones(&self) -> FingerBones {
        match self {
            FingerPose::Curls { curls, .. } => curls_to_bones(*curls),
            FingerPose::Bones { bones, .. } => *bones,
        }
    }

    /// Whether the fingers move as the trigger is pulled.
    pub fn has_trigger_pose(&self) -> bool {
        matches!(
            self,
            FingerPose::Curls {
                trigger_curls: Some(_),
                ..
            } | FingerPose::Bones {
                trigger_bones: Some(_),
                ..
            }
        )
    }

    /// This pose with its trigger pose set to the rest pose, index finger
    /// curled `amount` (0..=1) of the way toward a fist: pulling the trigger
    /// moves only the index; at 0 it moves nothing, so there is none. Curl
    /// grips stay curls. The VR grip workbench's Index steps and
    /// dark_explorer's trigger index slider both set it.
    pub fn with_trigger_index(&self, amount: f32) -> FingerPose {
        let amount = trigger_amount(amount);
        let pulled = amount > 0.0;
        match self {
            FingerPose::Curls { curls, .. } => {
                let mut pressed = *curls;
                pressed[1] = moved_curl(pressed[1], amount);
                FingerPose::Curls {
                    curls: *curls,
                    trigger_curls: pulled.then_some(pressed),
                }
            }
            FingerPose::Bones { bones, .. } => {
                let pressed = moved_bones(
                    bones,
                    &finger_bone_amounts(&[0.0, amount, 0.0, 0.0, 0.0]),
                    &open_right_hand().finger_bones(),
                    &fist_right_hand().finger_bones(),
                );
                FingerPose::Bones {
                    bones: *bones,
                    trigger_bones: pulled.then_some(pressed),
                }
            }
        }
    }

    /// How far the trigger pose curls the index from rest toward a fist
    /// (0..=1; a best fit for a captured pose); `None` with no trigger pose.
    pub fn trigger_index(&self) -> Option<f32> {
        match self {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => {
                let pressed = (*trigger_curls)?;
                let travel = 1.0 - curls[1];
                // A fully curled rest index has nowhere to go: it reads as there.
                Some(if travel <= 1e-6 {
                    1.0
                } else {
                    ((pressed[1] - curls[1]) / travel).clamp(0.0, 1.0)
                })
            }
            FingerPose::Bones {
                bones,
                trigger_bones,
            } => {
                let pressed = trigger_bones.as_ref()?;
                let fist = fist_right_hand().finger_bones();
                let error = |amount: f32| -> f32 {
                    FINGER_BONES[1]
                        .clone()
                        .map(|i| rotation_error(bones[i].slerp(fist[i], amount), pressed[i]))
                        .sum()
                };
                Some(minimize(error, 0.0, 1.0))
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => valid_finger_curls(curls) && trigger_curls.is_none_or(|c| valid_finger_curls(&c)),
            FingerPose::Bones {
                bones,
                trigger_bones,
            } => bones
                .iter()
                .chain(trigger_bones.iter().flatten())
                .all(|q| is_unit_quaternion(*q)),
        }
    }
}

fn finger_amounts([thumb, index, middle, ring, pinky]: [f32; 5]) -> FingerAmounts {
    FingerAmounts {
        thumb,
        index,
        middle,
        ring,
        pinky,
    }
}

/// The glove's own curl mapping: each finger blended open -> fist.
fn curls_to_bones(curls: [f32; 5]) -> FingerBones {
    open_right_hand()
        .blend_per_finger(&fist_right_hand(), &finger_amounts(curls))
        .finger_bones()
}

/// `FingerBones` indices per finger, thumb first (SteamVR bones 2..=25).
const FINGER_BONES: [std::ops::Range<usize>; 5] = [0..4, 4..9, 9..14, 14..19, 19..24];

/// `rest` with the index finger's bones taken from `index`.
pub fn with_index(rest: &FingerBones, index: &FingerBones) -> FingerBones {
    with_fingers(rest, index, [false, true, false, false, false])
}

/// `base` with the bones of each finger (thumb..little) `fingers` picks
/// taken from `from`, e.g. a tracked hand with its locked fingers stored.
pub fn with_fingers(base: &FingerBones, from: &FingerBones, fingers: [bool; 5]) -> FingerBones {
    let mut bones = *base;
    for (range, _) in FINGER_BONES.iter().zip(fingers).filter(|(_, take)| *take) {
        bones[range.clone()].copy_from_slice(&from[range.clone()]);
    }
    bones
}

/// Inverse of `curls_to_bones`: per finger, the curl whose open -> fist blend
/// is nearest `bones`. Exact for bones made from curls, a best fit otherwise.
fn bones_to_curls(bones: &FingerBones) -> [f32; 5] {
    let open = open_right_hand().finger_bones();
    let fist = fist_right_hand().finger_bones();
    std::array::from_fn(|finger| {
        // This finger's rotation error at `curl`, blended as `blend_per_finger`
        // does (cgmath's slerp takes the shortest arc itself).
        let error = |curl: f32| -> f32 {
            FINGER_BONES[finger]
                .clone()
                .map(|i| rotation_error(open[i].slerp(fist[i], curl), bones[i]))
                .sum()
        };
        let min = if finger == 0 { MIN_THUMB_CURL } else { 0.0 };
        minimize(error, min, 1.0)
    })
}

/// Monotone in the angle between two rotations; precise near zero, unlike acos.
fn rotation_error(a: Quaternion<f32>, b: Quaternion<f32>) -> f32 {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (a - b).magnitude().atan2((a + b).magnitude())
}

/// Minimum of `f` over `min..=max`: a coarse scan finds the basin (the error
/// need not be unimodal off the blend arc), golden-section search refines it.
fn minimize(f: impl Fn(f32) -> f32, min: f32, max: f32) -> f32 {
    const SAMPLES: usize = 40;
    let step = (max - min) / SAMPLES as f32;
    let (best, _) = (0..=SAMPLES)
        .map(|i| min + i as f32 * step)
        .map(|x| (x, f(x)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    let (mut lo, mut hi) = ((best - step).max(min), (best + step).min(max));
    let ratio = (5f32.sqrt() - 1.0) / 2.0;
    for _ in 0..32 {
        let a = hi - ratio * (hi - lo);
        let b = lo + ratio * (hi - lo);
        if f(a) < f(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    (lo + hi) / 2.0
}

fn blended_bones(rest: &FingerBones, pressed: Option<&FingerBones>, trigger: f32) -> FingerBones {
    let Some(pressed) = pressed else {
        return *rest;
    };
    let amount = trigger_amount(trigger);
    std::array::from_fn(|i| {
        // Slerp along the shortest arc
        let b = if rest[i].dot(pressed[i]) < 0.0 {
            -pressed[i]
        } else {
            pressed[i]
        };
        rest[i].slerp(b, amount)
    })
}

/// NaN fails the comparison, so non-finite rotations are rejected too.
fn is_unit_quaternion(q: Quaternion<f32>) -> bool {
    (q.magnitude2() - 1.0).abs() < 0.001
}

fn trigger_amount(trigger: f32) -> f32 {
    if trigger.is_finite() {
        trigger.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Curls of a pose the solver produced; the solver only emits curls.
fn fitted_curls(grip: &ResolvedGrip) -> &[f32; 5] {
    grip.fingers.rest_curls().expect("fits are curl poses")
}

/// Analog visual blend, independent of the fire/use action threshold.
pub fn blended_curls(rest: [f32; 5], pressed: Option<[f32; 5]>, trigger: f32) -> [f32; 5] {
    let amount = trigger_amount(trigger);
    let pressed = pressed.unwrap_or(rest);
    std::array::from_fn(|i| rest[i] + (pressed[i] - rest[i]) * amount)
}

/// How far off the item `GripSurface::tighten` leaves a finger: snug, never
/// through.
const TIGHTEN_CLEARANCE: f32 = 0.001 / crate::METERS_PER_WORLD_UNIT;
/// A phalanx this little past `TIGHTEN_CLEARANCE` already rests on the item:
/// a move it would make is no move.
const TIGHTEN_RESTING: f32 = 0.00005 / crate::METERS_PER_WORLD_UNIT;
/// Tighten scans a move in steps short enough that no part of the finger
/// travels past its own reach (its radius plus the clearance) in one, so a
/// blade thinner than a step is still met: at most this many, none shorter
/// than `TIGHTEN_MIN_STEP` of the move. Then it bisects.
const TIGHTEN_MAX_STEPS: usize = 2048;
const TIGHTEN_MIN_STEP: f32 = 1.0 / 4096.0;
const TIGHTEN_REFINE: usize = 12;

/// What `GripSurface::tighten` did with one finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FingerFit {
    /// Not asked to move.
    Skipped,
    /// Moved `amount` of the way toward a fist to rest `gap` (world units)
    /// off the item, nearest it at `contact` (item-local). Already snug:
    /// `amount` 0. `wrapped`: its joints past the phalanx that touched then
    /// curled on around the item.
    Touching {
        amount: f32,
        gap: f32,
        contact: [f32; 3],
        wrapped: bool,
    },
    /// Nothing in reach all the way to a fist: left as it was.
    NoContact,
    /// Through the item where it started, `depth` (world units, negative) at
    /// its deepest: left as it was.
    Buried { depth: f32 },
}

impl FingerFit {
    /// A word or two for what the finger did, for logs and readouts.
    pub fn outcome(&self) -> &'static str {
        match *self {
            FingerFit::Skipped => "skip",
            FingerFit::Touching {
                amount, wrapped, ..
            } => match (amount > 0.0, wrapped) {
                (true, false) => "closed",
                (true, true) => "closed+wrap",
                (false, false) => "snug",
                (false, true) => "snug+wrap",
            },
            FingerFit::NoContact => "none",
            FingerFit::Buried { .. } => "buried",
        }
    }
}

/// The fingers Tighten closes: all five at rest, the index onto the trigger
/// for the pressed pose, less any `locked` (thumb..little) by the editor.
pub fn tighten_fingers(pressed: bool, locked: [bool; 5]) -> [bool; 5] {
    std::array::from_fn(|finger| (!pressed || finger == 1) && !locked[finger])
}

/// `triangles` moved by `transform`, wound to keep facing out through a
/// reflection.
pub fn transformed_triangles(
    triangles: &[[Point3<f32>; 3]],
    transform: Matrix4<f32>,
) -> Vec<[Point3<f32>; 3]> {
    use cgmath::{SquareMatrix, Transform};
    let reflects = transform.determinant() < 0.0;
    triangles
        .iter()
        .map(|t| {
            let t = t.map(|p| transform.transform_point(p));
            if reflects { [t[0], t[2], t[1]] } else { t }
        })
        .collect()
}

/// A grip with its fingers closed onto the item, and what each finger did.
pub struct Tightened {
    pub grip: ResolvedGrip,
    pub fingers: [FingerFit; 5],
}

/// A posed finger as `GripSurface::tighten` tests it: per phalanx, from its
/// base to its tip, `PHALANX_SAMPLES` spheres it fills, swept between
/// neighbours (glove frame, authored size). Phalanx `k` of finger `f` turns
/// with `FINGER_BONES[f]` up to `PHALANX_BONES[f] + k`, as the glove's do:
/// the thumb's metacarpal first, the other fingers' from the knuckle.
#[derive(Clone, Debug)]
pub struct FingerChain {
    pub phalanges: Vec<[Sphere; PHALANX_SAMPLES]>,
}

pub const PHALANX_SAMPLES: usize = 5;

/// Each finger's first `FingerChain` phalanx's bone (`FingerBones` index).
pub(crate) const PHALANX_BONES: [usize; 5] = [0, 5, 10, 15, 20];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vector3<f32>,
    pub radius: f32,
}

impl FingerChain {
    /// Phalanges between neighbouring `joints`, as thick as `radius`
    /// everywhere.
    pub fn uniform(joints: &[Vector3<f32>], radius: f32) -> Self {
        let phalanges = joints
            .windows(2)
            .map(|ends| {
                std::array::from_fn(|i| {
                    let t = i as f32 / (PHALANX_SAMPLES - 1) as f32;
                    Sphere {
                        center: ends[0] * (1.0 - t) + ends[1] * t,
                        radius,
                    }
                })
            })
            .collect();
        FingerChain { phalanges }
    }
}

/// Per `FingerBones` bone, how far it moves along its own arc: toward the
/// fist (0..=1) or open (-1..0).
type BoneAmounts = [f32; FINGER_BONE_COUNT];

/// `grip` with its bones moved by `amounts`: the trigger pose's when
/// `pressed` (from the rest if there is none), else the rest's, which the
/// trigger pose's chosen `fingers` follow (its index only as far as the
/// rest's: pulling the trigger never opens it). A rest move sets the chosen
/// fingers' `contacts`. Curls stay curls while every finger moved as one.
fn moved_grip(
    grip: &ResolvedGrip,
    pressed: bool,
    fingers: [bool; 5],
    amounts: &BoneAmounts,
    contacts: [Option<[f32; 3]>; 5],
) -> ResolvedGrip {
    let open = open_right_hand().finger_bones();
    let fist = fist_right_hand().finger_bones();
    let follows = |f: usize| f != 1 && fingers[f];
    let whole: [f32; 5] = std::array::from_fn(|f| amounts[FINGER_BONES[f].start]);
    let on_curl_arcs = FINGER_BONES
        .iter()
        .all(|r| amounts[r.clone()].iter().all(|a| *a == amounts[r.start]));
    let fingers_pose = match &grip.fingers {
        FingerPose::Curls {
            curls,
            trigger_curls,
        } if on_curl_arcs => {
            let moved = |c: [f32; 5]| std::array::from_fn(|f| moved_curl(c[f], whole[f]));
            if pressed {
                let base = trigger_curls.unwrap_or(*curls);
                let pulled = moved(base);
                FingerPose::Curls {
                    curls: *curls,
                    trigger_curls: (trigger_curls.is_some() || pulled != base).then_some(pulled),
                }
            } else {
                let rest: [f32; 5] = moved(*curls);
                FingerPose::Curls {
                    curls: rest,
                    trigger_curls: trigger_curls.map(|pulled| {
                        std::array::from_fn(|f| match f {
                            1 if fingers[1] => pulled[1].max(rest[1]),
                            _ if follows(f) => rest[f],
                            _ => pulled[f],
                        })
                    }),
                }
            }
        }
        pose => {
            let FingerPose::Bones {
                bones,
                trigger_bones,
            } = pose.to_bones()
            else {
                unreachable!("to_bones gives bones")
            };
            if pressed {
                let base = trigger_bones.unwrap_or(bones);
                let pulled = moved_bones(&base, amounts, &open, &fist);
                FingerPose::Bones {
                    bones,
                    trigger_bones: (trigger_bones.is_some() || pulled != base).then_some(pulled),
                }
            } else {
                let rest = moved_bones(&bones, amounts, &open, &fist);
                FingerPose::Bones {
                    bones: rest,
                    trigger_bones: trigger_bones.map(|pulled| {
                        let behind =
                            fingers[1] && bones_to_curls(&pulled)[1] < bones_to_curls(&rest)[1];
                        let take = std::array::from_fn(|f| follows(f) || (f == 1 && behind));
                        with_fingers(&pulled, &rest, take)
                    }),
                }
            }
        }
    };
    // Contacts describe the rest pose.
    let contacts = std::array::from_fn(|f| match !pressed && fingers[f] {
        true => contacts[f],
        false => grip.contacts[f],
    });
    ResolvedGrip {
        fingers: fingers_pose,
        contacts,
        ..grip.clone()
    }
}

/// What `GripSurface::unbury` did with one finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FingerUnbury {
    /// Not asked to move.
    Skipped,
    /// Not through the item: left as it was.
    Clear,
    /// Was `depth` (world units, negative) through the item; opened
    /// `amount` (negative) of the way toward open to rest `gap` off it,
    /// nearest it at `contact` (item-local).
    Unburied {
        depth: f32,
        amount: f32,
        gap: f32,
        contact: [f32; 3],
    },
    /// Through the item `depth` deep even fully open: left as it was.
    Buried { depth: f32 },
}

impl FingerUnbury {
    /// A word for what the finger did, for logs and readouts.
    pub fn outcome(&self) -> &'static str {
        match self {
            FingerUnbury::Skipped => "skip",
            FingerUnbury::Clear => "clear",
            FingerUnbury::Unburied { .. } => "unburied",
            FingerUnbury::Buried { .. } => "buried",
        }
    }
}

/// A grip with its buried fingers opened out of the item, and what each
/// finger did.
pub struct Unburied {
    pub grip: ResolvedGrip,
    pub fingers: [FingerUnbury; 5],
}

/// A sample or segment of a phalanx `GripSurface::tighten` keeps off the
/// item (`GripSurface::finger_elements`).
#[derive(Clone, Copy, Debug)]
struct Element {
    gap: f32,
    contact: [f32; 3],
    phalanx: usize,
}

/// A finger's nearest element on phalanx `from` or later.
fn nearest(elements: &[Element], from: usize) -> Option<Element> {
    elements
        .iter()
        .filter(|e| e.phalanx >= from)
        .copied()
        .min_by(|a, b| a.gap.total_cmp(&b.gap))
}

/// Whether phalanx `from` or a later one is within `TIGHTEN_CLEARANCE`
/// (plus `margin`) of the item.
fn touching(elements: &[Element], from: usize, margin: f32) -> bool {
    nearest(elements, from).is_some_and(|e| e.gap <= TIGHTEN_CLEARANCE + margin)
}

/// One finger's bones `bones.0..bones.1` moving together along their arcs
/// from `from` to `to` (toward the fist when greater), moving its phalanges
/// from `phalanx` on.
#[derive(Clone, Copy)]
struct Move {
    bones: (usize, usize),
    from: f32,
    to: f32,
    phalanx: usize,
}

/// Where a `search` for one finger ended.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Found {
    /// It stopped between these amounts: the last before, the first at.
    Stop(f32, f32),
    /// It reached `to` without stopping.
    Reached,
    /// Out of steps first, at this amount, not stopped.
    Halted(f32),
}

/// For each finger `moves` moves (the hand otherwise at `base`), where
/// `stop` first holds for its posed chain. Scanned in steps no sphere moves
/// past its reach in (a step that would is halved), then bisected.
fn search(
    chains: &mut impl FnMut(&BoneAmounts) -> [FingerChain; 5],
    base: &BoneAmounts,
    moves: &[Option<Move>; 5],
    stop: &dyn Fn(&FingerChain, Move) -> bool,
) -> [Option<Found>; 5] {
    let posed = |amount: &dyn Fn(usize, Move) -> f32| {
        let mut amounts = *base;
        for (f, m) in moves.iter().enumerate() {
            if let Some(m) = *m {
                amounts[m.bones.0..m.bones.1].fill(amount(f, m));
            }
        }
        amounts
    };
    let moving = |chain: &FingerChain, m: Move| {
        chain.phalanges[m.phalanx..]
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
    };
    // A first step from the spheres' whole arcs (at most pi/2 times their
    // chord, for one joint) over their reach.
    let (now, end) = (chains(&posed(&|_, m| m.from)), chains(&posed(&|_, m| m.to)));
    let mut step: [f32; 5] = std::array::from_fn(|f| {
        moves[f].map_or(0.0, |m| {
            let steps = moving(&now[f], m)
                .iter()
                .zip(moving(&end[f], m))
                .map(|(a, b)| {
                    let arc = (b.center - a.center).magnitude() * std::f32::consts::FRAC_PI_2;
                    arc / (a.radius + TIGHTEN_CLEARANCE)
                })
                .fold(1.0, f32::max);
            (m.to - m.from) / steps.ceil()
        })
    });
    // Per finger: how far it has scanned without stopping, its spheres
    // there, and where it ended.
    let mut at: [f32; 5] = std::array::from_fn(|f| moves[f].map_or(0.0, |m| m.from));
    let mut last: [Vec<Sphere>; 5] =
        std::array::from_fn(|f| moves[f].map_or(Vec::new(), |m| moving(&now[f], m)));
    let mut found: [Option<Found>; 5] =
        moves.map(|m| m.and_then(|m| (m.from == m.to).then_some(Found::Reached)));
    for _ in 0..TIGHTEN_MAX_STEPS {
        let stepping: [bool; 5] = std::array::from_fn(|f| moves[f].is_some() && found[f].is_none());
        if !stepping.contains(&true) {
            break;
        }
        let next: [f32; 5] = std::array::from_fn(|f| match moves[f] {
            Some(m) if m.to > m.from => (at[f] + step[f]).min(m.to),
            Some(m) => (at[f] + step[f]).max(m.to),
            None => 0.0,
        });
        let posed_at = chains(&posed(&|f, m| match stepping[f] {
            true => next[f],
            false => m.from,
        }));
        for f in (0..5).filter(|&f| stepping[f]) {
            let m = moves[f].unwrap();
            let spheres = moving(&posed_at[f], m);
            let too_far = spheres
                .iter()
                .zip(&last[f])
                .any(|(a, b)| (a.center - b.center).magnitude() > b.radius + TIGHTEN_CLEARANCE);
            if too_far && step[f].abs() > TIGHTEN_MIN_STEP {
                step[f] *= 0.5;
            } else if stop(&posed_at[f], m) {
                found[f] = Some(Found::Stop(at[f], next[f]));
            } else {
                (at[f], last[f]) = (next[f], spheres);
                if next[f] == m.to {
                    found[f] = Some(Found::Reached);
                }
            }
        }
    }
    for f in 0..5 {
        if moves[f].is_some() && found[f].is_none() {
            found[f] = Some(Found::Halted(at[f]));
        }
    }
    for _ in 0..TIGHTEN_REFINE {
        let refining = found.map(|found| matches!(found, Some(Found::Stop(..))));
        if !refining.contains(&true) {
            break;
        }
        let middle = |f: usize| match found[f] {
            Some(Found::Stop(before, at)) => (before + at) * 0.5,
            _ => 0.0,
        };
        let posed_at = chains(&posed(&|f, m| match refining[f] {
            true => middle(f),
            false => m.from,
        }));
        for f in (0..5).filter(|&f| refining[f]) {
            let Some(Found::Stop(before, at)) = found[f].as_mut() else {
                continue;
            };
            let m = (*before + *at) * 0.5;
            if stop(&posed_at[f], moves[f].unwrap()) {
                *at = m;
            } else {
                *before = m;
            }
        }
    }
    found
}

/// `bones` moved by `amounts`, each along its own arc toward `fist` or
/// `open`, so a captured shape keeps its character.
fn moved_bones(
    bones: &FingerBones,
    amounts: &BoneAmounts,
    open: &FingerBones,
    fist: &FingerBones,
) -> FingerBones {
    std::array::from_fn(|i| moved_bone(bones[i], amounts[i], open[i], fist[i]))
}

/// `bone` moved `amount` along its arc toward `fist` (or toward `open`).
fn moved_bone(
    bone: Quaternion<f32>,
    amount: f32,
    open: Quaternion<f32>,
    fist: Quaternion<f32>,
) -> Quaternion<f32> {
    match amount {
        0.0 => bone,
        amount if amount > 0.0 => bone.slerp(fist, amount),
        amount => bone.slerp(open, -amount),
    }
}

/// One amount per finger, for each of its bones.
fn finger_bone_amounts(amounts: &[f32; 5]) -> BoneAmounts {
    let mut bones = [0.0; crate::hand_pose::FINGER_BONE_COUNT];
    for (range, amount) in FINGER_BONES.iter().zip(amounts) {
        bones[range.clone()].fill(*amount);
    }
    bones
}

/// `moved_bones` for a curl: the open -> fist blend is a slerp per bone,
/// so moving along it is linear in the curl.
fn moved_curl(curl: f32, amount: f32) -> f32 {
    if amount >= 0.0 {
        curl + amount * (1.0 - curl)
    } else {
        curl * (1.0 + amount)
    }
}

pub struct GripSurface {
    mesh: TriMesh,
    min: Vector3<f32>,
    max: Vector3<f32>,
    closed: bool,
}

impl GripSurface {
    pub fn new(triangles: &[[Point3<f32>; 3]]) -> Option<Self> {
        if triangles.is_empty() {
            return None;
        }
        let mut min = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = -min;
        let mut vertices = Vec::with_capacity(triangles.len() * 3);
        for triangle in triangles {
            for p in triangle {
                if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
                    return None;
                }
                for axis in 0..3 {
                    min[axis] = min[axis].min(p[axis]);
                    max[axis] = max[axis].max(p[axis]);
                }
                vertices.push(na::Point3::new(p.x, p.y, p.z));
            }
        }
        let indices = (0..triangles.len() as u32)
            .map(|i| [3 * i, 3 * i + 1, 3 * i + 2])
            .collect();
        let mesh = TriMesh::with_flags(
            vertices,
            indices,
            TriMeshFlags::MERGE_DUPLICATE_VERTICES | TriMeshFlags::ORIENTED,
        )
        .ok()?;
        let mut edges = std::collections::HashMap::new();
        for triangle in mesh.indices() {
            for (a, b) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let edge = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
                edge.0 += 1;
                edge.1 += if a < b { 1 } else { -1 };
            }
        }
        let closed = edges
            .values()
            .all(|&(count, orientation)| count == 2 && orientation == 0);
        let center = (min + max) * 0.5;
        let volume: f32 = triangles
            .iter()
            .map(|t| {
                let a = vec3(t[0].x, t[0].y, t[0].z) - center;
                let b = vec3(t[1].x, t[1].y, t[1].z) - center;
                let c = vec3(t[2].x, t[2].y, t[2].z) - center;
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        // Double-sided picture tiles pair every edge but enclose no solid.
        // An inside test on those coplanar faces rejects otherwise valid grips.
        let closed = closed && volume.abs() > f32::EPSILON * (max - min).magnitude().powi(3);
        tracing::info!(
            closed,
            volume,
            triangles = triangles.len(),
            "VR grip bake surface"
        );
        Some(Self {
            mesh,
            min,
            max,
            closed,
        })
    }

    /// Bounded candidate search. Face rays select actual visible surfaces,
    /// including concave handles; AABBs only seed rays, never finger contacts.
    pub fn resolve(&self, hand: &GripKinematics, hints: &GripHints) -> Option<ResolvedGrip> {
        if !hints.is_valid() {
            return None;
        }
        let extent = self.max - self.min;
        if extent.magnitude2() < 1e-8 {
            return None;
        }
        let center = (self.max + self.min) * 0.5;
        let mut sorted = [extent.x, extent.y, extent.z];
        sorted.sort_by(f32::total_cmp);
        let family = if sorted[0] < sorted[2] * 0.2 {
            "pinch"
        } else if sorted[2] > sorted[1] * 1.5 {
            "cylindrical"
        } else {
            "broad"
        };
        let family = hints
            .pose_family
            .map(|hint| match hint {
                0 => "cylindrical",
                1 => "pinch",
                2 => "broad",
                3 => "trigger",
                _ => family,
            })
            .unwrap_or(family);
        let mut best: Option<ResolvedGrip> = None;
        // A broad sheet needs an edge grip: the original interior-only grid
        // can put the thumb through it even at the largest palm clearance.
        let samples: &[f32] = if sorted[0] < sorted[2] * 0.2 {
            &[-0.98, -0.7, 0.0, 0.7, 0.98]
        } else {
            &[-0.7, 0.0, 0.7]
        };
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let mut outward = vec3(0.0, 0.0, 0.0);
                outward[axis] = sign;
                for &u in samples {
                    for &v in samples {
                        let mut origin = center;
                        origin[axis] += sign * (extent[axis] * 0.5 + 0.1);
                        origin[(axis + 1) % 3] += u * extent[(axis + 1) % 3] * 0.5;
                        origin[(axis + 2) % 3] += v * extent[(axis + 2) % 3] * 0.5;
                        let ray = Ray::new(
                            na::Point3::new(origin.x, origin.y, origin.z),
                            na::Vector3::new(-outward.x, -outward.y, -outward.z),
                        );
                        let Some(hit) = self.mesh.cast_local_ray_and_get_normal(
                            &ray,
                            extent[axis] + 0.2,
                            false,
                        ) else {
                            continue;
                        };
                        let anchor = origin - outward * hit.time_of_impact;
                        if hints.anchor_region.is_some_and(|[min, max]| {
                            (0..3).any(|axis| anchor[axis] < min[axis] || anchor[axis] > max[axis])
                        }) {
                            continue;
                        }
                        let outward = vec3(hit.normal.x, hit.normal.y, hit.normal.z).normalize();
                        if hints.keep_upright && (anchor.y - center.y).abs() > extent.y * 0.25 {
                            continue;
                        }
                        for twist in [0.0, 90.0, 180.0, 270.0] {
                            let rotation =
                                Quaternion::from_axis_angle(hand.normal, cgmath::Deg(twist))
                                    * Quaternion::from_arc(-outward, hand.normal, None);
                            if hints.keep_upright
                                && rotation.rotate_vector(Vector3::unit_y()).y < 0.8
                            {
                                continue;
                            }
                            if hints.upright_axis.is_some_and(|[x, y, z]| {
                                rotation.rotate_vector(vec3(x, y, z).normalize()).y < 0.8
                            }) {
                                continue;
                            }
                            for clearance in [1.0, 2.0, 3.0, 5.0, 8.0] {
                                let offset = hand.palm - rotation.rotate_vector(anchor)
                                    + hand.normal * FINGER_RADIUS * clearance;
                                let mut resolved = self.fit_at(hand, offset, rotation);
                                if hints.keep_upright {
                                    // Useful grips on upright vessels sit around the
                                    // body, rather than maximizing contact on the rim/base.
                                    resolved.score -=
                                        ((anchor.y - center.y) / extent.y.max(0.001)).abs() * 6.0;
                                }
                                if !resolved.is_valid() {
                                    continue;
                                }
                                // Families select the grasp objective; they share
                                // rig-derived finger arcs rather than per-item poses.
                                match family {
                                    "pinch" => {
                                        for finger in [0, 1] {
                                            resolved.score += if resolved.contacts[finger].is_some()
                                            {
                                                2.0
                                            } else {
                                                -2.0
                                            };
                                        }
                                    }
                                    "cylindrical" => {
                                        let curls = &fitted_curls(&resolved)[1..];
                                        let min = curls.iter().copied().fold(1.0, f32::min);
                                        let max = curls.iter().copied().fold(0.0, f32::max);
                                        resolved.score -= (max - min) * 0.5;
                                    }
                                    "trigger" => resolved.score -= fitted_curls(&resolved)[1] * 0.5,
                                    _ => {
                                        resolved.score -=
                                            fitted_curls(&resolved).iter().sum::<f32>() * 0.05
                                    }
                                }
                                resolved.pose_family = family.to_owned();
                                resolved.anchor = [anchor.x, anchor.y, anchor.z];
                                resolved.score -= (u * u + v * v) * 0.02
                                    + rotation.v.magnitude2() * 0.15
                                    + (clearance - 1.0) * 0.02;
                                if best.as_ref().is_none_or(|b| resolved.score > b.score) {
                                    best = Some(resolved);
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut result = best?;
        if hints.anchor.is_some() || hints.rotation.is_some() {
            let anchor = hints.anchor.unwrap_or(result.anchor);
            let rotation = hints
                .rotation
                .map(|[w, x, y, z]| Quaternion::new(w, x, y, z).normalize())
                .unwrap_or(result.rotation);
            let offset = hand.palm - rotation.rotate_vector(vec3(anchor[0], anchor[1], anchor[2]))
                + hand.normal * FINGER_RADIUS;
            result = self.fit_at(hand, offset, rotation);
            result.anchor = anchor;
            result.pose_family = family.to_owned();
        }
        for (finger, amount) in hints.curls.iter().enumerate() {
            if let Some(amount) = amount {
                result
                    .fingers
                    .rest_curls_mut()
                    .expect("fits are curl poses")[finger] = *amount;
                let step = (amount * CURL_STEPS as f32).round() as usize;
                result.contacts[finger] = hand.fingers[finger][step].iter().find_map(|sample| {
                    let local = result
                        .rotation
                        .conjugate()
                        .rotate_vector(*sample - result.offset);
                    let p = na::Point3::new(local.x, local.y, local.z);
                    let projection = self.mesh.project_local_point_and_get_feature(&p).0;
                    ((p - projection.point).norm() <= FINGER_RADIUS).then_some([
                        projection.point.x,
                        projection.point.y,
                        projection.point.z,
                    ])
                });
            }
        }
        (result.is_valid()
            && self.pose_is_clear(hand, result.offset, result.rotation, fitted_curls(&result)))
        .then_some(result)
    }

    /// Search only around authored hand surfaces, preserving the gun's aim axis.
    /// Used offline; the runtime consumes the resulting prepared pose.
    pub fn resolve_weapon(
        &self,
        hand: &GripKinematics,
        arm: &[Point3<f32>],
        seed_offset: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Option<ResolvedGrip> {
        let mut best: Option<ResolvedGrip> = None;
        let stride = (arm.len() / 60).max(1);
        for point in arm.iter().step_by(stride) {
            if ![point.x, point.y, point.z].into_iter().all(f32::is_finite) {
                continue;
            }
            let offset = hand.palm - rotation.rotate_vector(point.to_vec());
            let drift = (offset - seed_offset).magnitude();
            // Reload hands and forearms far from the known grip are not candidates.
            if drift > 0.4 {
                continue;
            }
            for clearance in [-0.015, 0.0, 0.015, 0.03] {
                let offset = offset + hand.normal * clearance;
                let local_palm = rotation.conjugate().rotate_vector(hand.palm - offset);
                let palm = na::Point3::new(local_palm.x, local_palm.y, local_palm.z);
                if (palm - self.mesh.project_local_point_and_get_feature(&palm).0.point).norm()
                    > 0.065
                {
                    continue;
                }
                let mut candidate = self.fit_at(hand, offset, rotation);
                candidate.score -= drift * 3.0;
                // Prefer wrapping the handle over balancing its butt on nearly open fingers.
                candidate.score += fitted_curls(&candidate)[2..].iter().sum::<f32>() * 0.7;
                candidate.pose_family = "trigger".to_owned();
                candidate.anchor = [point.x, point.y, point.z];
                if candidate.is_valid() && best.as_ref().is_none_or(|b| candidate.score > b.score) {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    fn pose_is_clear(
        &self,
        hand: &GripKinematics,
        offset: Vector3<f32>,
        rotation: Quaternion<f32>,
        curls: &[f32; 5],
    ) -> bool {
        let inverse = rotation.conjugate();
        for (finger, amount) in curls.iter().enumerate() {
            let samples = &hand.fingers[finger][(amount * CURL_STEPS as f32).round() as usize];
            for sample in samples {
                let local = inverse.rotate_vector(*sample - offset);
                let p = na::Point3::new(local.x, local.y, local.z);
                let projection = self.mesh.project_local_point_and_get_feature(&p).0;
                if (self.closed && projection.is_inside)
                    || (p - projection.point).norm() < FINGER_RADIUS * 0.6
                {
                    return false;
                }
            }
            for pair in samples.windows(2) {
                let a = inverse.rotate_vector(pair[0] - offset);
                let b = inverse.rotate_vector(pair[1] - offset);
                let delta = b - a;
                if delta.magnitude2() > 1e-10 {
                    let ray = Ray::new(
                        na::Point3::new(a.x, a.y, a.z),
                        na::Vector3::new(delta.x, delta.y, delta.z),
                    );
                    if self.mesh.cast_local_ray(&ray, 1.0, false).is_some() {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn fit_at(
        &self,
        hand: &GripKinematics,
        offset: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> ResolvedGrip {
        let inverse = rotation.conjugate();
        let mut curls = [1.0; 5];
        let mut contacts = [None; 5];
        let palm = inverse.rotate_vector(hand.palm - offset);
        let p = na::Point3::new(palm.x, palm.y, palm.z);
        let projection = self.mesh.project_local_point_and_get_feature(&p).0;
        let mut invalid = (self.closed && projection.is_inside)
            || (p - projection.point).norm() < FINGER_RADIUS * 0.8;
        let mut score = 0.0;
        for (finger, curve) in hand.fingers.iter().enumerate() {
            let mut closest_tip = (f32::INFINITY, 0);
            'curl: for (step, samples) in curve.iter().enumerate() {
                let tip = inverse.rotate_vector(*samples.last().unwrap() - offset);
                let p = na::Point3::new(tip.x, tip.y, tip.z);
                let distance =
                    (p - self.mesh.project_local_point_and_get_feature(&p).0.point).norm();
                if distance < closest_tip.0 {
                    closest_tip = (distance, step);
                }
                for sample in samples.iter().rev() {
                    let local = inverse.rotate_vector(*sample - offset);
                    let p = na::Point3::new(local.x, local.y, local.z);
                    let projection = self.mesh.project_local_point_and_get_feature(&p).0;
                    let distance = (p - projection.point).norm();
                    let inside = self.closed && projection.is_inside;
                    if step == 0 && (inside || distance < FINGER_RADIUS * 0.8) {
                        invalid = true;
                        break 'curl;
                    }
                    if inside || distance <= FINGER_RADIUS {
                        curls[finger] = step.saturating_sub(1) as f32 / CURL_STEPS as f32;
                        contacts[finger] =
                            Some([projection.point.x, projection.point.y, projection.point.z]);
                        // Contact at the open pose means the item was seated
                        // through a finger, not that the finger grasped it.
                        score += if step == 0 {
                            -3.0
                        } else {
                            1.0 + if finger == 0 { 0.5 } else { 0.0 }
                        };
                        break 'curl;
                    }
                }
            }
            if contacts[finger].is_none() {
                curls[finger] = closest_tip.1 as f32 / CURL_STEPS as f32;
                score -= closest_tip.0.min(0.3);
            }
        }
        invalid |= !self.pose_is_clear(hand, offset, rotation, &curls);
        if invalid {
            score = f32::NEG_INFINITY;
        }
        ResolvedGrip {
            item_scale: 1.0,
            pose_family: "broad".to_owned(),
            offset,
            rotation,
            hand_rotation: Quaternion::one(),
            fingers: FingerPose::curls(curls),
            contacts,
            anchor: [0.0; 3],
            score,
        }
    }

    /// Close each finger chosen in `fingers` onto the item, moving each joint
    /// along its own arc from its current shape toward a fist, so a captured
    /// shape is kept. Proximal to distal: the whole finger until it touches,
    /// then each joint past the knuckle on its own until the phalanx it
    /// moves, or a later one, touches too (or it reaches the fist). Touching
    /// rests a phalanx `TIGHTEN_CLEARANCE` off the item; one already resting
    /// on it moves no further.
    /// - Through the item where it starts: buried, left exactly as it is.
    /// - Nothing in reach all the way to a fist: left.
    ///
    /// The placement is untouched; curls stay curls unless a wrap bent a
    /// finger off its curl arc. `pressed` tightens the trigger pose (from the
    /// rest if there is none) and leaves the rest; otherwise the rest, which
    /// a trigger pose's chosen fingers follow (its index only as far as the
    /// rest's). `chains` poses each finger for a bone pose (`FingerChain`);
    /// `grip` is in authored units.
    pub fn tighten(
        &self,
        grip: &ResolvedGrip,
        pressed: bool,
        fingers: [bool; 5],
        mut chains: impl FnMut(&FingerBones) -> [FingerChain; 5],
    ) -> Tightened {
        let start = grip
            .fingers
            .at(if pressed { 1.0 } else { 0.0 })
            .rest_bones();
        let open = open_right_hand().finger_bones();
        let fist = fist_right_hand().finger_bones();
        let mut posed = |amounts: &BoneAmounts| chains(&moved_bones(&start, amounts, &open, &fist));
        let mut amounts = [0.0; FINGER_BONE_COUNT];
        let at_start = posed(&amounts);
        let buried: [Option<f32>; 5] =
            std::array::from_fn(|f| fingers[f].then(|| self.depth(grip, &at_start[f])).flatten());
        // Fingers still closing, and those that have touched.
        let mut closing: [bool; 5] = std::array::from_fn(|f| fingers[f] && buried[f].is_none());
        let mut touched = [false; 5];
        let mut wrapped = [false; 5];
        let phalanges = at_start.each_ref().map(|chain| chain.phalanges.len());
        for stage in 0..phalanges.into_iter().max().unwrap_or(0) {
            // Stage 0 moves the whole finger; stage k the joint before phalanx k.
            let now = posed(&amounts);
            let mut moves = [None; 5];
            for f in (0..5).filter(|&f| closing[f] && stage < phalanges[f]) {
                if stage > 0 && !touched[f] {
                    continue;
                }
                let elements = self.finger_elements(grip, &now[f]);
                if touching(&elements, stage, TIGHTEN_RESTING) {
                    touched[f] = true;
                    continue;
                }
                let bones = match stage {
                    0 => (FINGER_BONES[f].start, FINGER_BONES[f].end),
                    _ => (PHALANX_BONES[f] + stage, PHALANX_BONES[f] + stage + 1),
                };
                debug_assert!(bones.1 <= FINGER_BONES[f].end, "finger {f} phalanx {stage}");
                moves[f] = Some(Move {
                    bones,
                    from: amounts[bones.0],
                    to: 1.0,
                    phalanx: stage,
                });
            }
            if moves.iter().all(Option::is_none) {
                continue;
            }
            let touches = |chain: &FingerChain, m: Move| {
                touching(&self.finger_elements(grip, chain), m.phalanx, 0.0)
            };
            let found = search(&mut posed, &amounts, &moves, &touches);
            for f in 0..5 {
                let (Some(m), Some(found)) = (moves[f], found[f]) else {
                    continue;
                };
                // Whether the bones it moves turn at all on the way.
                let turns = |to: f32| {
                    (m.bones.0..m.bones.1).any(|b| {
                        let at = |amount| moved_bone(start[b], amount, open[b], fist[b]);
                        rotation_error(at(amounts[b]), at(to)) > 1e-4
                    })
                };
                match (found, stage) {
                    // Nothing in reach (or out of steps): the finger is left.
                    (Found::Reached | Found::Halted(_), 0) => closing[f] = false,
                    // Nothing more to touch: on to the fist.
                    (Found::Reached, _) => {
                        wrapped[f] |= turns(m.to);
                        amounts[m.bones.0..m.bones.1].fill(m.to);
                    }
                    // Out of steps: stop there.
                    (Found::Halted(at), _) => {
                        wrapped[f] |= turns(at);
                        amounts[m.bones.0..m.bones.1].fill(at);
                        closing[f] = false;
                    }
                    (Found::Stop(clear, _), _) => {
                        wrapped[f] |= stage > 0 && turns(clear);
                        amounts[m.bones.0..m.bones.1].fill(clear);
                        touched[f] = true;
                    }
                }
            }
        }
        let at = posed(&amounts);
        let closed: [f32; 5] = std::array::from_fn(|f| amounts[FINGER_BONES[f].start]);
        let mut result = [FingerFit::Skipped; 5];
        for f in (0..5).filter(|&f| fingers[f]) {
            let nearest = nearest(&self.finger_elements(grip, &at[f]), 0);
            result[f] = match (buried[f], nearest) {
                (Some(depth), _) => FingerFit::Buried { depth },
                (None, Some(e)) if touched[f] => FingerFit::Touching {
                    amount: closed[f],
                    gap: e.gap,
                    contact: e.contact,
                    wrapped: wrapped[f],
                },
                _ => FingerFit::NoContact,
            };
        }
        let contacts = std::array::from_fn(|f| match result[f] {
            FingerFit::Touching { contact, .. } => Some(contact),
            FingerFit::Buried { .. } => grip.contacts[f],
            _ => None,
        });
        Tightened {
            grip: moved_grip(grip, pressed, fingers, &amounts, contacts),
            fingers: result,
        }
    }

    /// Open each finger chosen in `fingers` that is through the item (as
    /// `tighten` finds it buried) out of it: the whole finger along its own
    /// arc toward open until all of it is `TIGHTEN_CLEARANCE` off the item,
    /// so `tighten` finds it resting there. Fingers clear of the item are
    /// left, as is one still through it fully open. The placement, slot and
    /// trigger pose following are as `tighten`'s.
    pub fn unbury(
        &self,
        grip: &ResolvedGrip,
        pressed: bool,
        fingers: [bool; 5],
        mut chains: impl FnMut(&FingerBones) -> [FingerChain; 5],
    ) -> Unburied {
        let start = grip
            .fingers
            .at(if pressed { 1.0 } else { 0.0 })
            .rest_bones();
        let open = open_right_hand().finger_bones();
        let fist = fist_right_hand().finger_bones();
        let mut posed = |amounts: &BoneAmounts| chains(&moved_bones(&start, amounts, &open, &fist));
        let at_start = posed(&[0.0; FINGER_BONE_COUNT]);
        let depth: [Option<f32>; 5] =
            std::array::from_fn(|f| fingers[f].then(|| self.depth(grip, &at_start[f])).flatten());
        // Through the item even fully open: no search frees it.
        let fully_open = posed(&finger_bone_amounts(
            &depth.map(|d| if d.is_some() { -1.0 } else { 0.0 }),
        ));
        let moves: [Option<Move>; 5] = std::array::from_fn(|f| {
            (depth[f].is_some() && self.clear_of(grip, &fully_open[f])).then_some(Move {
                bones: (FINGER_BONES[f].start, FINGER_BONES[f].end),
                from: 0.0,
                to: -1.0,
                phalanx: 0,
            })
        });
        let clear = |chain: &FingerChain, _| self.clear_of(grip, chain);
        let found = search(&mut posed, &[0.0; FINGER_BONE_COUNT], &moves, &clear);
        // Opened to the first clear amount.
        let opened: [Option<f32>; 5] = found.map(|found| match found {
            Some(Found::Stop(_, clear)) => Some(clear),
            _ => None,
        });
        let amounts = finger_bone_amounts(&opened.map(|a| a.unwrap_or(0.0)));
        let at = posed(&amounts);
        let result: [FingerUnbury; 5] = std::array::from_fn(|f| {
            let nearest = nearest(&self.finger_elements(grip, &at[f]), 0);
            match (fingers[f], depth[f], opened[f], nearest) {
                (false, ..) => FingerUnbury::Skipped,
                (true, None, ..) => FingerUnbury::Clear,
                (true, Some(depth), Some(amount), Some(e)) => FingerUnbury::Unburied {
                    depth,
                    amount,
                    gap: e.gap,
                    contact: e.contact,
                },
                (true, Some(depth), ..) => FingerUnbury::Buried { depth },
            }
        });
        let contacts = std::array::from_fn(|f| match result[f] {
            FingerUnbury::Unburied { contact, .. } => Some(contact),
            _ => grip.contacts[f],
        });
        Unburied {
            grip: moved_grip(grip, pressed, fingers, &amounts, contacts),
            fingers: result,
        }
    }

    /// Whether all of `chain` is `TIGHTEN_CLEARANCE` off the item and none
    /// of it inside: the elements first, the costlier inside test only once
    /// they are clear.
    fn clear_of(&self, grip: &ResolvedGrip, chain: &FingerChain) -> bool {
        self.finger_elements(grip, chain)
            .iter()
            .all(|e| e.gap >= TIGHTEN_CLEARANCE)
            && !chain
                .phalanges
                .iter()
                .flatten()
                .any(|sphere| self.inside(self.item_local(grip, sphere.center)))
    }

    /// A glove-frame point in item-local space.
    fn item_local(&self, grip: &ResolvedGrip, p: Vector3<f32>) -> na::Point3<f32> {
        let q = grip.rotation.conjugate().rotate_vector(p - grip.offset) / grip.item_scale;
        na::Point3::new(q.x, q.y, q.z)
    }

    /// How deep `chain` is through the item (world units, negative), if it
    /// is: an element within its radius of the surface (by how much), or a
    /// sphere's centre further inside the item than that (its distance in,
    /// plus its radius).
    fn depth(&self, grip: &ResolvedGrip, chain: &FingerChain) -> Option<f32> {
        // Only a centre further in than its radius needs the inside test:
        // nearer, its element already reads it through.
        let inside = chain.phalanges.iter().flatten().filter_map(|sphere| {
            let p = self.item_local(grip, sphere.center);
            let surface = self.mesh.project_local_point(&p, false).point;
            let distance = (p - surface).norm() * grip.item_scale;
            (distance >= sphere.radius && self.inside(p)).then(|| -distance - sphere.radius)
        });
        self.finger_elements(grip, chain)
            .iter()
            .map(|e| e.gap)
            .chain(inside)
            .filter(|gap| *gap < 0.0)
            .min_by(f32::total_cmp)
    }

    /// Whether item-local `p` is inside the item, by its generalized winding
    /// number: overlapping closed parts count as solid, and an open part
    /// elsewhere does not spoil a closed one, unlike a sign read off the
    /// nearest face. Assumes each part's triangles wind one way (either way:
    /// the sign is ignored); a part wound inconsistently cancels out. The
    /// auto-fit keeps its `closed` test so baked grips stay as they are.
    fn inside(&self, p: na::Point3<f32>) -> bool {
        if (0..3).any(|axis| p[axis] < self.min[axis] || p[axis] > self.max[axis]) {
            return false;
        }
        let solid_angles: f32 = self
            .mesh
            .triangles()
            .map(|t| {
                let (a, b, c) = (t.a - p, t.b - p, t.c - p);
                let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
                let volume = a.dot(&b.cross(&c));
                let d = la * lb * lc + a.dot(&b) * lc + b.dot(&c) * la + c.dot(&a) * lb;
                2.0 * volume.atan2(d)
            })
            .sum();
        // Near a whole turn: a closed part. The mouth of an open part winds
        // partway and counts as outside; deep in a narrow open one, it nears
        // a whole turn and counts as inside.
        (solid_angles / (4.0 * std::f32::consts::PI)).abs() > 0.75
    }

    /// A finger's elements, phalanx by phalanx: its spheres (glove frame),
    /// then the segments between their centres. Each is how far it is off
    /// the item's surface in world units past the finger's radius there, and
    /// the item-local point nearest it. Negative is within the radius (a
    /// segment crossing the surface: -radius); a clear segment is infinitely
    /// far.
    fn finger_elements(&self, grip: &ResolvedGrip, chain: &FingerChain) -> Vec<Element> {
        let identity = na::Isometry3::identity();
        let mut elements = Vec::with_capacity(2 * PHALANX_SAMPLES * chain.phalanges.len());
        for (phalanx, spheres) in chain.phalanges.iter().enumerate() {
            let points = spheres.map(|sphere| self.item_local(grip, sphere.center));
            let radii = spheres.map(|sphere| sphere.radius);
            for (p, radius) in points.iter().zip(radii) {
                let projection = self.mesh.project_local_point(p, false);
                let distance = (p - projection.point).norm() * grip.item_scale;
                let point = projection.point;
                elements.push(Element {
                    gap: distance - radius,
                    contact: [point.x, point.y, point.z],
                    phalanx,
                });
            }
            // Between samples a thin edge (a trigger) can come within the
            // radius of the finger's axis; crossing the axis is -radius.
            // Farther than a radius past the finger is past any touch.
            for (pair, radius) in points.windows(2).zip(radii.windows(2)) {
                let reach = 2.0 * radius[0].max(radius[1]) / grip.item_scale;
                let segment = Segment::new(pair[0], pair[1]);
                let contact = query::contact(&identity, &segment, &identity, &self.mesh, reach);
                elements.push(match contact {
                    Ok(Some(contact)) => {
                        // The finger's radius where the segment is nearest. Between
                        // spheres of unequal size the deepest point can lie a little
                        // off it: a few tenths of a millimetre on the glove's tapers.
                        let t = (contact.point1 - pair[0]).norm()
                            / (pair[1] - pair[0]).norm().max(f32::EPSILON);
                        let radius = radius[0] + (radius[1] - radius[0]) * t.min(1.0);
                        Element {
                            gap: contact.dist.max(0.0) * grip.item_scale - radius,
                            contact: [contact.point2.x, contact.point2.y, contact.point2.z],
                            phalanx,
                        }
                    }
                    _ => Element {
                        gap: f32::INFINITY,
                        contact: [0.0; 3],
                        phalanx,
                    },
                });
            }
        }
        elements
    }
}

/// Shared boundary conversion for both the rendered glove and fitting samples.
pub fn glove_to_hand(hand: Handedness) -> Matrix4<f32> {
    hand.mirror()
        * Matrix4::from_angle_y(cgmath::Deg(180.0))
        * Matrix4::from_scale(crate::hand_glove::GLOVE_SCALE)
}

/// Authoring hints are inputs to the offline baker. Numbers are stable:
/// 0 cylindrical, 1 pinch, 2 broad grasp, 3 trigger. Omission stays automatic.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GripHints {
    pub pose_family: Option<u8>,
    pub keep_upright: bool,
    /// Item-local direction that should point up in the calibrated hand pose.
    /// Unlike keep_upright, this does not restrict the anchor height.
    pub upright_axis: Option<[f32; 3]>,
    /// Optional inclusive item-local bounds for candidate palm contact points.
    pub anchor_region: Option<[[f32; 3]; 2]>,
    /// Optional item-local palm contact anchor and item-to-hand rotation [w,x,y,z].
    pub anchor: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    /// Override selected fingers; null retains automatic contact fitting.
    pub curls: [Option<f32>; 5],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BakedGripEntry {
    /// Edited in Explorer; bulk baking must preserve this pose.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub authored: bool,
    pub model: String,
    pub hand: String,
    pub surface_hash: String,
    pub kinematics_hash: String,
    pub hints_hash: String,
    pub grip: ResolvedGrip,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GripLibrary {
    pub version: u32,
    pub solver_revision: u32,
    pub entries: Vec<BakedGripEntry>,
    /// How many leading entries came from the headset overrides (`prepend`).
    #[serde(skip)]
    pub headset: usize,
    /// Every model is looked up, and every entry matched, by its class's
    /// canonical model: one grip holds the whole class.
    #[serde(skip)]
    pub classes: GripClasses,
}

impl GripLibrary {
    /// Prepared poses are selected by model identity (its class's canonical
    /// model) and hand. Fit fingerprints and solver revisions are provenance,
    /// never runtime eligibility gates.
    pub fn lookup(&self, model: &str, hand: &str) -> Option<&ResolvedGrip> {
        if self.version != 1 {
            return None;
        }
        let model = self.classes.canonical(model);
        self.entries
            .iter()
            .find(|entry| entry.model == model && entry.hand == hand && entry.grip.is_valid())
            .map(|entry| &entry.grip)
    }

    /// The grip `hand` holds `model` with, and whether it is the other hand's,
    /// to mirror. For a `mirrorable` item the other hand's grip stands in when
    /// it comes from a higher source: one posed on the headset outranks this
    /// hand's shipped grip, so posing one hand poses both. This hand's own
    /// entry wins within a source.
    pub fn select(
        &self,
        model: &str,
        hand: Handedness,
        mirrorable: bool,
    ) -> Option<(&ResolvedGrip, bool)> {
        let name = |hand| match hand {
            Handedness::Left => "left",
            Handedness::Right => "right",
        };
        if !mirrorable {
            return self.lookup(model, name(hand)).map(|grip| (grip, false));
        }
        if self.version != 1 {
            return None;
        }
        let other = hand.other();
        let model = self.classes.canonical(model);
        let (headset, shipped) = self.entries.split_at(self.headset.min(self.entries.len()));
        [headset, shipped].into_iter().find_map(|source| {
            let find = |hand| {
                source
                    .iter()
                    .find(|e| e.model == model && e.hand == name(hand) && e.grip.is_valid())
                    .map(|e| &e.grip)
            };
            find(hand)
                .map(|grip| (grip, false))
                .or_else(|| find(other).map(|grip| (grip, true)))
        })
    }

    /// `select`'s grip for `hand`, mirrored if it is the other hand's, and
    /// whether it was. `model_mirror` is the item's right-to-left frame
    /// (`vr_weapon_grip::model_mirror`); `None` for an item drawn alike in
    /// either hand, which only ever takes its own hand's grip.
    pub fn lookup_for(
        &self,
        model: &str,
        hand: Handedness,
        model_mirror: Option<Matrix4<f32>>,
    ) -> Option<(ResolvedGrip, bool)> {
        let (grip, other) = self.select(model, hand, model_mirror.is_some())?;
        if !other {
            return Some((grip.clone(), false));
        }
        // A grip that will not mirror leaves this hand its own, if any.
        model_mirror
            .and_then(|mirror| grip.mirrored(hand_model_mirror(mirror, hand)?))
            .map(|mirrored| (mirrored, true))
            .or_else(|| self.lookup_for(model, hand, None))
    }

    /// Remove the first entry for `model`'s class and `hand` (a staged one:
    /// staging prepends).
    pub fn remove_first(&mut self, model: &str, hand: &str) {
        let model = self.classes.canonical(model);
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.model == model && e.hand == hand)
        {
            self.entries.remove(index);
            if index < self.headset {
                self.headset -= 1;
            }
        }
    }

    /// Make `entry` the only headset entry for its model's class and hand, as
    /// a save leaves the overrides file.
    pub fn replace_headset(&mut self, entry: BakedGripEntry) {
        let model = self.classes.canonical(&entry.model).to_owned();
        let mut index = 0;
        while index < self.headset {
            let e = &self.entries[index];
            if e.model == model && e.hand == entry.hand {
                self.entries.remove(index);
                self.headset -= 1;
            } else {
                index += 1;
            }
        }
        self.prepend(GripLibrary {
            entries: vec![entry],
            ..Default::default()
        });
    }
}

/// Poses saved on the headset by the in-VR pose tool, under the data root.
/// Entries use library keys (e.g. `vr_magazine::grip_model`), not fit keys.
pub const GRIP_OVERRIDES_FILE: &str = "vr-grip-overrides.json";

/// Shipped grip libraries in runtime lookup order: the first model/hand match wins.
pub const GRIP_RESOURCES: [&str; 4] = [
    "vr-grips.json",
    "vr-weapon-grips.json",
    "vr-tricorder-grips.json",
    "vr-magazine-grips.json",
];

/// Models that share geometry, and so share one grip.
pub const GRIP_CLASSES_FILE: &str = "vr-grip-classes.json";

/// Grip classes: each is a list of models with the same geometry, whose first
/// (canonical) model's grip entry holds them all. A model outside every class
/// is its own canonical model.
#[derive(Clone, Default, Debug)]
pub struct GripClasses {
    classes: Vec<Vec<String>>,
    /// Each member's class index.
    index: std::collections::HashMap<String, usize>,
}

impl GripClasses {
    /// A version 1 classes file. A model in two classes (or twice in one) or
    /// a class of fewer than two models makes the whole file invalid.
    pub fn parse(text: &str) -> Result<GripClasses, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            version: u32,
            classes: Vec<Vec<String>>,
        }
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.version != 1 {
            return Err(format!("version_{}", file.version));
        }
        let mut index = std::collections::HashMap::new();
        for (i, class) in file.classes.iter().enumerate() {
            if class.len() < 2 {
                return Err(format!("class {i} has fewer than two models"));
            }
            for model in class {
                if index.insert(model.clone(), i).is_some() {
                    return Err(format!("{model} is in more than one class"));
                }
            }
        }
        Ok(GripClasses {
            classes: file.classes,
            index,
        })
    }

    /// `parse`, logging the outcome; a missing or invalid file is no classes.
    /// Logs to stdout: the Quest runtime has no tracing subscriber.
    pub fn load(text: Option<&str>) -> GripClasses {
        match text.map(Self::parse) {
            Some(Ok(classes)) => {
                println!(
                    "SHOCK2QUEST_VR_GRIP_CLASSES classes={}",
                    classes.classes.len()
                );
                classes
            }
            Some(Err(error)) => {
                println!("SHOCK2QUEST_VR_GRIP_CLASSES ignored=\"{error}\"");
                GripClasses::default()
            }
            None => GripClasses::default(),
        }
    }

    /// The model whose grip entry `model` uses: its class's first.
    pub fn canonical<'a>(&'a self, model: &'a str) -> &'a str {
        self.members(model).map_or(model, |class| &class[0])
    }

    /// `model`'s class, canonical model first, if it is in one.
    pub fn members(&self, model: &str) -> Option<&[String]> {
        self.index.get(model).map(|&i| &self.classes[i][..])
    }

    /// How a pose tool names `model`'s shared grip, as two lines, if it is
    /// in a class: ["Shared by 6 models", "(canonical: medpatch)"].
    pub fn label(&self, model: &str) -> Option<[String; 2]> {
        self.members(model).map(|members| {
            [
                format!("Shared by {} models", members.len()),
                format!("(canonical: {})", members[0]),
            ]
        })
    }

    /// `entries` keyed by their class's canonical model, one per model and
    /// hand: of two entries for one (say two members posed on an older
    /// build), the later wins, and the earlier is logged.
    pub fn canonicalize(&self, entries: Vec<BakedGripEntry>) -> Vec<BakedGripEntry> {
        let mut kept: Vec<(String, BakedGripEntry)> = Vec::with_capacity(entries.len());
        for mut entry in entries {
            let canonical = self.canonical(&entry.model).to_owned();
            let posed = std::mem::replace(&mut entry.model, canonical);
            if let Some(i) = kept
                .iter()
                .position(|(_, e)| e.model == entry.model && e.hand == entry.hand)
            {
                let (earlier, _) = kept.remove(i);
                println!(
                    "SHOCK2QUEST_VR_GRIP_CLASSES replaced={earlier} by={posed} hand={} class={}",
                    entry.hand, entry.model
                );
            }
            kept.push((posed, entry));
        }
        kept.into_iter().map(|(_, entry)| entry).collect()
    }
}

impl GripLibrary {
    /// Put `overrides` ahead of every entry, keyed by canonical model
    /// (`GripClasses::canonicalize`); `lookup` takes the first match.
    pub fn prepend(&mut self, overrides: GripLibrary) {
        let entries = self.classes.canonicalize(overrides.entries);
        self.headset += entries.len();
        self.entries.splice(0..0, entries);
    }
}

/// The overrides file (`GRIP_OVERRIDES_FILE`): grip entries ahead of the
/// shipped libraries, and captured support poses by model over the authored
/// support profiles. A file without `support` is a plain grip library.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct GripOverrides {
    #[serde(flatten)]
    pub grips: GripLibrary,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub support: std::collections::BTreeMap<String, crate::vr_support::SupportPose>,
    /// Entries and support poses this build cannot read, verbatim: a save
    /// writes them back, as they may be another build's work.
    #[serde(skip)]
    pub unread: UnreadOverrides,
}

#[derive(Clone, Default)]
pub struct UnreadOverrides {
    pub entries: Vec<serde_json::Value>,
    pub support: serde_json::Map<String, serde_json::Value>,
}

impl GripOverrides {
    /// A missing file is normal; an unreadable one is logged and ignored.
    /// Logs to stdout: the Quest runtime has no tracing subscriber.
    pub fn load(path: &std::path::Path) -> Option<GripOverrides> {
        let text = std::fs::read_to_string(path).ok()?;
        let path = path.display();
        match Self::parse(&text) {
            Ok(overrides) => {
                let entries = overrides.grips.entries.len();
                let support = overrides.support.len();
                let unread = overrides.unread.entries.len() + overrides.unread.support.len();
                println!(
                    "SHOCK2QUEST_VR_GRIP_OVERRIDES path={path} entries={entries} support={support} unread={unread}"
                );
                Some(overrides)
            }
            Err(error) => {
                println!("SHOCK2QUEST_VR_GRIP_OVERRIDES path={path} ignored=\"{error}\"");
                None
            }
        }
    }

    /// Read a version 1 overrides file entry by entry: an entry or support
    /// pose that does not decode is logged and kept in `unread`. Only text
    /// that is not such a file at all fails.
    pub fn parse(text: &str) -> Result<GripOverrides, String> {
        use serde_json::Value;
        let mut value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let object = value.as_object_mut().ok_or("not an object")?;
        let entries = match object.insert("entries".to_owned(), Value::Array(Vec::new())) {
            Some(Value::Array(entries)) => entries,
            None => Vec::new(),
            Some(_) => return Err("entries is not a list".to_owned()),
        };
        let support = match object.remove("support") {
            Some(Value::Object(support)) => support,
            None => serde_json::Map::new(),
            Some(_) => return Err("support is not an object".to_owned()),
        };
        let mut overrides: GripOverrides =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        if overrides.grips.version != 1 {
            return Err(format!("version_{}", overrides.grips.version));
        }
        for entry in entries {
            match serde_json::from_value(entry.clone()) {
                Ok(entry) => overrides.grips.entries.push(entry),
                Err(error) => {
                    println!("SHOCK2QUEST_VR_GRIP_OVERRIDES unread_entry=\"{error}\"");
                    overrides.unread.entries.push(entry);
                }
            }
        }
        for (model, pose) in support {
            match serde_json::from_value(pose.clone()) {
                Ok(pose) => {
                    overrides.support.insert(model, pose);
                }
                Err(error) => {
                    println!(
                        "SHOCK2QUEST_VR_GRIP_OVERRIDES unread_support={model} error=\"{error}\""
                    );
                    overrides.unread.support.insert(model, pose);
                }
            }
        }
        Ok(overrides)
    }

    /// Replace the grip of `entry`'s model's class (in `classes`) and hand:
    /// the readable entries and any unread ones.
    pub fn replace_grip(&mut self, entry: BakedGripEntry, classes: &GripClasses) {
        let model = classes.canonical(&entry.model);
        let hand = entry.hand.as_str();
        self.unread.entries.retain(|raw| {
            raw["hand"] != hand
                || raw["model"]
                    .as_str()
                    .is_none_or(|m| classes.canonical(m) != model)
        });
        let entries = &mut self.grips.entries;
        entries.retain(|e| e.hand != hand || classes.canonical(&e.model) != model);
        entries.push(entry);
    }

    /// The file's JSON, `unread` written back after the readable entries (a
    /// readable support pose wins its model's key).
    pub fn to_json(&self) -> serde_json::Result<Vec<u8>> {
        use serde_json::Value;
        let mut value = serde_json::to_value(self)?;
        let object = value
            .as_object_mut()
            .expect("a struct serializes as an object");
        if let Some(Value::Array(entries)) = object.get_mut("entries") {
            entries.extend(self.unread.entries.iter().cloned());
        }
        let unread_support: serde_json::Map<_, _> = self
            .unread
            .support
            .iter()
            .filter(|(model, _)| !self.support.contains_key(*model))
            .map(|(model, pose)| (model.clone(), pose.clone()))
            .collect();
        if !unread_support.is_empty()
            && let Value::Object(support) = object
                .entry("support")
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
        {
            support.extend(unread_support);
        }
        let mut bytes = serde_json::to_vec_pretty(&value)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

impl Default for GripLibrary {
    fn default() -> Self {
        Self {
            version: 1,
            solver_revision: SOLVER_REVISION,
            entries: Vec::new(),
            headset: 0,
            classes: GripClasses::default(),
        }
    }
}

/// Stable hashes invalidate a bake when either the visible mesh or glove's
/// sampled pose arcs change. Quantize below visible precision (0.076 mm) so
/// host/Quest floating-point differences and signed zero do not invalidate a bake.
fn fingerprint(values: impl IntoIterator<Item = f32>) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for value in values {
        for byte in ((value * 10_000.0).round() as i64).to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

pub fn surface_fingerprint(triangles: &[[Point3<f32>; 3]]) -> String {
    fingerprint(triangles.iter().flatten().flat_map(|p| [p.x, p.y, p.z]))
}

impl GripKinematics {
    pub(crate) fn palm_in_world(&self) -> Vector3<f32> {
        self.palm * crate::world_scale::WorldScale::active().personal()
    }
    pub fn fingerprint(&self) -> String {
        fingerprint(
            self.fingers
                .iter()
                .flatten()
                .flatten()
                .copied()
                .chain([self.palm, self.normal])
                .flat_map(|p| [p.x, p.y, p.z]),
        )
    }
}

impl GripHints {
    pub fn fingerprint(&self) -> String {
        fingerprint(
            [
                self.pose_family.unwrap_or(255) as f32,
                self.keep_upright as u8 as f32,
                self.anchor_region.is_some() as u8 as f32,
                self.anchor.is_some() as u8 as f32,
                self.rotation.is_some() as u8 as f32,
            ]
            .into_iter()
            .chain(
                self.anchor_region
                    .unwrap_or([[0.0; 3]; 2])
                    .into_iter()
                    .flatten(),
            )
            .chain(self.anchor.unwrap_or([0.0; 3]))
            .chain(self.rotation.unwrap_or([0.0; 4]))
            .chain(
                self.curls
                    .iter()
                    .flat_map(|v| [v.is_some() as u8 as f32, v.unwrap_or(0.0)]),
            )
            // An absent axis leaves the previous search and its fingerprint
            // unchanged. Opting in adds three values and invalidates that bake.
            .chain(
                self.upright_axis
                    .map(|[x, y, z]| {
                        let axis = vec3(x, y, z).normalize();
                        [axis.x, axis.y, axis.z]
                    })
                    .into_iter()
                    .flatten(),
            ),
        )
    }

    pub fn is_valid(&self) -> bool {
        self.anchor_region.is_none_or(|[min, max]| {
            (0..3).all(|i| min[i].is_finite() && max[i].is_finite() && min[i] <= max[i])
        }) && self.upright_axis.is_none_or(|axis| {
            let length_squared = axis.iter().map(|v| v * v).sum::<f32>();
            axis.iter().all(|v| v.is_finite())
                && length_squared.is_finite()
                && length_squared > 1e-8
        }) && self.pose_family.is_none_or(|p| p <= 3)
            && self
                .anchor
                .iter()
                .flatten()
                .chain(self.rotation.iter().flatten())
                .all(|n| n.is_finite())
            && self
                .rotation
                .is_none_or(|r| r.iter().map(|v| v * v).sum::<f32>() > 1e-8)
            && self
                .curls
                .iter()
                .flatten()
                .all(|n| n.is_finite() && (0.0..=1.0).contains(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{One, point3};

    fn plane(z: f32) -> GripSurface {
        GripSurface::new(&[
            [
                point3(-1.0, -1.0, z),
                point3(1.0, -1.0, z),
                point3(1.0, 1.0, z),
            ],
            [
                point3(-1.0, -1.0, z),
                point3(1.0, 1.0, z),
                point3(-1.0, 1.0, z),
            ],
        ])
        .unwrap()
    }

    fn straight_fingers() -> GripKinematics {
        GripKinematics {
            fingers: std::array::from_fn(|_| {
                (0..=CURL_STEPS)
                    .map(|i| vec![vec3(0.0, 0.0, i as f32 * 0.005)])
                    .collect()
            }),
            palm: vec3(0.0, 0.0, 0.0),
            normal: Vector3::unit_z(),
        }
    }

    #[test]
    fn fingers_stop_before_crossing_a_thin_surface() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        assert!(
            fitted_curls(&grip)
                .iter()
                .all(|c| (*c - 8.0 / 24.0).abs() < 1e-6)
        );
        assert!(
            grip.contacts
                .iter()
                .all(|p| (p.unwrap()[2] - 0.05).abs() < 1e-6)
        );
    }

    #[test]
    fn double_sided_picture_has_no_solid_interior() {
        // Retail score-picture tiles triangulate the two faces with opposite
        // diagonals. Every edge is paired, but the sheet encloses no volume.
        let a = point3(-0.22, -0.22, 0.05);
        let b = point3(-0.22, 0.22, 0.05);
        let c = point3(0.22, -0.22, 0.05);
        let d = point3(0.22, 0.22, 0.05);
        let surface = GripSurface::new(&[[b, a, d], [d, a, c], [a, b, c], [c, b, d]]).unwrap();
        assert!(!surface.closed, "paired coplanar faces are not a solid");
        let grip = surface.fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        assert!(grip.is_valid());
        assert!(grip.contacts.iter().all(Option::is_some));
        let edge = GripHints {
            anchor_region: Some([[0.20, -0.22, 0.049], [0.22, 0.22, 0.051]]),
            ..Default::default()
        };
        assert!(surface.resolve(&straight_fingers(), &edge).is_some());
    }

    #[test]
    fn unreachable_fingers_use_nearest_pose_instead_of_closing_into_empty_space() {
        let grip = plane(-0.5).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        assert_eq!(fitted_curls(&grip), &[0.0; 5]);
        assert_eq!(grip.contacts, [None; 5]);
    }

    #[test]
    fn changing_item_origin_does_not_change_finger_contact() {
        let hand = straight_fingers();
        let a = plane(0.05).fit_at(&hand, vec3(0.0, 0.0, 0.0), Quaternion::one());
        let b = plane(1.05).fit_at(&hand, vec3(0.0, 0.0, -1.0), Quaternion::one());
        assert_eq!(a.fingers, b.fingers);
        for (a, b) in a.contacts.iter().zip(b.contacts) {
            assert!((a.unwrap()[2] + 1.0 - b.unwrap()[2]).abs() < 1e-6);
        }
    }

    #[test]
    fn invalid_surfaces_and_overrides_are_rejected() {
        assert!(GripSurface::new(&[]).is_none());
        assert!(GripSurface::new(&[[point3(f32::NAN, 0.0, 0.0); 3]]).is_none());
        let hints = GripHints {
            rotation: Some([0.0; 4]),
            ..Default::default()
        };
        assert!(!hints.is_valid());
        let hints = GripHints {
            curls: [Some(1.2), None, None, None, None],
            ..Default::default()
        };
        assert!(!hints.is_valid());
    }

    #[test]
    fn fingerprints_ignore_subprecision_drift_but_detect_geometry_changes() {
        assert_eq!(fingerprint([0.012345, 0.0]), fingerprint([0.0123451, -0.0]));
        assert_ne!(fingerprint([0.012345]), fingerprint([0.022345]));
    }

    #[test]
    fn serialized_prepared_grips_round_trip_and_reject_invalid_rotation() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let mut restored: ResolvedGrip =
            serde_json::from_str(&serde_json::to_string(&grip).unwrap()).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.fingers, grip.fingers);
        restored.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        assert!(!restored.is_valid());
    }
    #[test]
    fn extended_thumb_round_trips_and_blends_without_extending_other_fingers() {
        let mut grip =
            plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        grip.fingers = FingerPose::Curls {
            curls: [-0.5, 1.0, 1.0, 1.0, 1.0],
            trigger_curls: Some([-1.0, 0.8, 1.0, 1.0, 1.0]),
        };
        let mut restored: ResolvedGrip =
            serde_json::from_str(&serde_json::to_string(&grip).unwrap()).unwrap();
        assert!(restored.is_valid());
        assert_eq!(
            restored.fingers.curls_at(0.5),
            Some([-0.75, 0.9, 1.0, 1.0, 1.0])
        );
        let curls = restored.fingers.rest_curls_mut().unwrap();
        curls[0] = -1.01;
        assert!(!restored.is_valid());
        let curls = restored.fingers.rest_curls_mut().unwrap();
        curls[0] = -0.5;
        curls[1] = -0.1;
        assert!(!restored.is_valid());
    }
    /// The trigger index is the rest pose with only the index curled toward
    /// the fist's: rest at 0, the fist at 1, farther from rest as it grows,
    /// and it reads back. A curl grip stays curls.
    #[test]
    fn the_trigger_index_curls_only_the_index_from_rest_toward_a_fist() {
        let rest_curls = [0.1, 0.2, 0.4, 0.5, 0.6];
        let rest = curls_to_bones(rest_curls);
        let fist = fist_right_hand().finger_bones();
        let pose = FingerPose::Bones {
            bones: rest,
            trigger_bones: None,
        };
        assert_eq!(pose.trigger_index(), None);
        assert_eq!(
            pose.with_trigger_index(0.0),
            pose,
            "0%: the trigger moves nothing, so there is no trigger pose"
        );
        let index = FINGER_BONES[1].clone();
        let mut last = 0.0;
        for step in 1..=10 {
            let amount = step as f32 / 10.0;
            let set = pose.with_trigger_index(amount);
            let FingerPose::Bones {
                bones,
                trigger_bones: Some(pressed),
            } = set
            else {
                panic!("{step}: bones with a trigger pose, got {set:?}");
            };
            assert_eq!(bones, rest, "the rest pose is kept");
            for i in (0..rest.len()).filter(|i| !index.contains(i)) {
                assert_eq!(pressed[i], rest[i], "{step}: bone {i} is not the index's");
            }
            let travel: f32 = index
                .clone()
                .map(|i| rotation_error(rest[i], pressed[i]))
                .sum();
            assert!(travel > last, "{step}: {travel} after {last}");
            last = travel;
            let off = |target: &FingerBones| -> f32 {
                index
                    .clone()
                    .map(|i| rotation_error(pressed[i], target[i]))
                    .sum()
            };
            if step == 10 {
                assert!(off(&fist) < 1e-3, "100% is the fist's index");
            }
            let read = set.trigger_index().unwrap();
            assert!((read - amount).abs() < 1e-3, "{read} vs {amount}");
        }
        let curls = FingerPose::curls(rest_curls).with_trigger_index(0.5);
        let FingerPose::Curls {
            curls: rest_kept,
            trigger_curls: Some(pressed),
        } = curls
        else {
            panic!("curls with a trigger pose, got {curls:?}");
        };
        assert_eq!(rest_kept, rest_curls);
        assert!((pressed[1] - 0.6).abs() < 1e-6, "{pressed:?}");
        assert_eq!(
            [pressed[0], pressed[2], pressed[3], pressed[4]],
            [0.1, 0.4, 0.5, 0.6]
        );
        assert!((curls.trigger_index().unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(curls.with_trigger_index(0.0), FingerPose::curls(rest_curls));
    }

    #[test]
    fn with_index_takes_only_the_index_bones() {
        let rest = curls_to_bones([0.1; 5]);
        let other = curls_to_bones([0.9; 5]);
        let mixed = with_index(&rest, &other);
        for i in 0..rest.len() {
            let expected = if FINGER_BONES[1].contains(&i) {
                other[i]
            } else {
                rest[i]
            };
            assert_eq!(mixed[i], expected, "bone {i}");
        }
    }

    #[test]
    fn with_fingers_takes_each_picked_fingers_bones() {
        let base = curls_to_bones([0.1; 5]);
        let other = curls_to_bones([0.9; 5]);
        let picked = [true, false, false, true, true];
        let mixed = with_fingers(&base, &other, picked);
        for (finger, bones) in FINGER_BONES.iter().enumerate() {
            for i in bones.clone() {
                let expected = if picked[finger] { other[i] } else { base[i] };
                assert_eq!(mixed[i], expected, "finger {finger} bone {i}");
            }
        }
    }

    #[test]
    fn tighten_closes_the_slots_fingers_never_a_locked_one() {
        let none = [false; 5];
        assert_eq!(tighten_fingers(false, none), [true; 5]);
        assert_eq!(
            tighten_fingers(true, none),
            [false, true, false, false, false]
        );
        let locked = [true, true, false, false, false];
        assert_eq!(
            tighten_fingers(false, locked),
            [false, false, true, true, true]
        );
        assert_eq!(tighten_fingers(true, locked), none);
    }

    #[test]
    fn trigger_curls_blend_independently_and_round_trip() {
        let mut grip =
            plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let rest = [0.2, 0.4, 0.6, 0.8, 1.0];
        grip.fingers = FingerPose::curls(rest);
        assert_eq!(grip.fingers.curls_at(1.0), Some(rest));
        let legacy = serde_json::to_value(&grip).unwrap();
        assert!(legacy.get("trigger_curls").is_none());
        assert_eq!(
            serde_json::from_value::<ResolvedGrip>(legacy)
                .unwrap()
                .fingers,
            FingerPose::curls(rest)
        );
        let pressed = [0.8, 0.8, 0.6, 0.8, 1.0];
        let with_pressed = |pressed| FingerPose::Curls {
            curls: rest,
            trigger_curls: Some(pressed),
        };
        grip.fingers = with_pressed(pressed);
        assert_eq!(grip.fingers.curls_at(-1.0), Some(rest));
        assert_eq!(grip.fingers.curls_at(f32::NAN), Some(rest));
        assert_eq!(grip.fingers.curls_at(2.0), Some(pressed));
        let halfway = grip.fingers.curls_at(0.5).unwrap();
        assert!((halfway[0] - 0.5).abs() < 1e-6);
        assert!((halfway[1] - 0.6).abs() < 1e-6);
        assert_eq!(&halfway[2..], &rest[2..]);
        let restored: ResolvedGrip =
            serde_json::from_str(&serde_json::to_string(&grip).unwrap()).unwrap();
        assert_eq!(restored, grip);
        assert!(restored.is_valid());
        grip.fingers = with_pressed([1.1; 5]);
        assert!(!grip.is_valid());
        grip.fingers = with_pressed([f32::NAN; 5]);
        assert!(!grip.is_valid());
    }

    #[test]
    fn device_overrides_win_lookup_and_bad_files_are_ignored() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let entry = |scale| BakedGripEntry {
            authored: true,
            model: "mug".into(),
            hand: "left".into(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip: grip.clone().with_item_scale(scale),
        };
        let mut library = GripLibrary::default();
        library.entries.push(entry(1.0));
        let dir = crate::test_support::TempDir::new("grip-overrides");
        let path = dir.path().join(GRIP_OVERRIDES_FILE);
        assert!(GripOverrides::load(&path).is_none(), "missing file");
        std::fs::write(&path, "{not json").unwrap();
        assert!(GripOverrides::load(&path).is_none(), "corrupt file");
        let mut overrides = GripLibrary::default();
        overrides.entries.push(entry(0.5));
        std::fs::write(&path, serde_json::to_string(&overrides).unwrap()).unwrap();
        let loaded = GripOverrides::load(&path).unwrap();
        assert!(
            loaded.support.is_empty(),
            "a file from before support poses"
        );
        library.prepend(loaded.grips);
        assert_eq!(library.lookup("mug", "left").unwrap().item_scale, 0.5);
    }

    /// One entry this build cannot read (a hand edit, or a newer build's)
    /// loses only itself, and is written back as it was.
    #[test]
    fn an_unreadable_override_entry_is_kept_aside_not_fatal() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let entry = |model: &str| BakedGripEntry {
            authored: true,
            model: model.into(),
            hand: "left".into(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip: grip.clone(),
        };
        let pose = crate::vr_support::SupportPose {
            palm_anchor: [0.1, 0.0, 0.0],
            rotation_degrees: [0.0; 3],
            fingers: FingerPose::curls([0.5; 5]),
        };
        let bad_entry = serde_json::json!({"model": "sg_h", "hand": "left", "grip": 7});
        let bad_pose = serde_json::json!({"palm_anchor": "here"});
        let mut file = serde_json::to_value(GripOverrides {
            grips: GripLibrary {
                entries: vec![entry("mug")],
                ..Default::default()
            },
            support: [("sg_h".to_owned(), pose.clone())].into(),
            ..Default::default()
        })
        .unwrap();
        file["entries"]
            .as_array_mut()
            .unwrap()
            .push(bad_entry.clone());
        file["support"]["wrench_h"] = bad_pose.clone();
        let dir = crate::test_support::TempDir::new("grip-overrides-unread");
        let path = dir.path().join(GRIP_OVERRIDES_FILE);
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();

        let mut loaded = GripOverrides::load(&path).expect("the readable rest loads");
        assert_eq!(loaded.grips.entries.len(), 1);
        assert_eq!(loaded.support, [("sg_h".to_owned(), pose)].into());

        // Written back verbatim; a new readable pose for the same key wins.
        loaded.grips.entries.push(entry("cup"));
        let written: serde_json::Value =
            serde_json::from_slice(&loaded.to_json().unwrap()).unwrap();
        assert_eq!(written["entries"].as_array().unwrap().len(), 3);
        assert_eq!(written["entries"][2], bad_entry);
        assert_eq!(written["support"]["wrench_h"], bad_pose);
        loaded
            .support
            .insert("wrench_h".to_owned(), loaded.support["sg_h"].clone());
        let written: serde_json::Value =
            serde_json::from_slice(&loaded.to_json().unwrap()).unwrap();
        assert_eq!(written["support"]["wrench_h"], written["support"]["sg_h"]);
        assert!(
            GripOverrides::parse("[1]").is_err(),
            "not an overrides file"
        );
    }

    /// Bone rotations equal up to quaternion sign.
    fn assert_bones_eq(a: &FingerBones, b: &FingerBones) {
        for (bone, (a, b)) in a.iter().zip(b).enumerate() {
            assert!(a.dot(*b).abs() > 1.0 - 1e-5, "bone {bone}: {a:?} vs {b:?}");
        }
    }

    #[test]
    fn curls_convert_to_the_bones_the_glove_draws() {
        let rest = [-0.5, 0.3, 0.6, 0.8, 1.0];
        let pressed = [-1.0, 1.0, 0.6, 0.8, 1.0];
        let curls = FingerPose::Curls {
            curls: rest,
            trigger_curls: Some(pressed),
        };
        let bones = curls.to_bones();
        assert!(bones.is_valid());
        let drawn = |fingers: &FingerPose, trigger| match fingers.hand_shape_at(trigger) {
            HandShape::Curls(amounts) => open_right_hand()
                .blend_per_finger(&fist_right_hand(), &amounts)
                .finger_bones(),
            HandShape::Pose(pose) => pose.finger_bones(),
        };
        for trigger in [0.0, 1.0] {
            assert_bones_eq(&drawn(&bones, trigger), &drawn(&curls, trigger));
        }
        // Mid-trigger is a real in-between pose.
        let mid = drawn(&bones, 0.5);
        let index = 6 - 2;
        assert!(mid[index + 1].dot(drawn(&bones, 0.0)[index + 1]).abs() < 1.0 - 1e-4);
        assert_eq!(bones.to_bones(), bones);
    }

    #[test]
    fn bones_made_from_curls_convert_back_to_those_curls() {
        let rests = [
            [0.0; 5],
            [1.0; 5],
            [MIN_THUMB_CURL, 0.0, 0.0, 0.0, 0.0],
            [-0.5, 1.0, 1.0, 1.0, 1.0],
            [0.2, 0.4, 0.6, 0.8, 1.0],
            [0.65, 0.25, 0.7, 0.35, 0.9],
        ];
        let close = |a: [f32; 5], b: [f32; 5]| {
            assert!(
                a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-3),
                "{a:?} vs {b:?}"
            );
        };
        for rest in rests {
            for pressed in [None, Some([1.0, 0.9, 0.5, 0.1, 0.0])] {
                let pose = FingerPose::Curls {
                    curls: rest,
                    trigger_curls: pressed,
                };
                let FingerPose::Curls {
                    curls,
                    trigger_curls,
                } = pose.to_bones().to_curls()
                else {
                    panic!("expected curls");
                };
                close(curls, rest);
                assert_eq!(trigger_curls.is_some(), pressed.is_some());
                if let (Some(a), Some(b)) = (trigger_curls, pressed) {
                    close(a, b);
                }
            }
        }
        assert_eq!(
            FingerPose::curls([0.3; 5]).to_curls(),
            FingerPose::curls([0.3; 5])
        );
    }

    #[test]
    fn a_tracked_hand_converts_to_valid_curls() {
        use crate::hand_capture::{
            fit_hand,
            tests::{openxr_right_hand, pose_rig},
        };
        let estimate = |index_curl| {
            let bones = fit_hand(&openxr_right_hand(index_curl), &pose_rig(Handedness::Right))
                .unwrap()
                .bones;
            let pose = FingerPose::Bones {
                bones,
                trigger_bones: Some(bones),
            }
            .to_curls();
            assert!(pose.is_valid(), "{pose:?}");
            *pose.rest_curls().unwrap()
        };
        // Curling the tracked index curls the estimated one.
        assert!(estimate(0.8)[1] > estimate(0.0)[1] + 0.2);
    }

    #[test]
    fn bone_poses_round_trip_and_reject_unnormalized_rotations() {
        let mut grip =
            plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        grip.fingers = FingerPose::curls([0.2, 0.4, 0.6, 0.8, 1.0]).to_bones();
        let json = serde_json::to_value(&grip).unwrap();
        assert!(json.get("curls").is_none() && json.get("trigger_bones").is_none());
        let restored: ResolvedGrip = serde_json::from_value(json).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored, grip);
        assert_eq!(restored.fingers.curls_at(0.0), None);
        let FingerPose::Bones { bones, .. } = &mut grip.fingers else {
            unreachable!()
        };
        bones[3] = Quaternion::new(2.0, 0.0, 0.0, 0.0);
        assert!(!grip.is_valid());
    }

    #[test]
    fn grips_mixing_curls_and_bones_are_rejected() {
        let bones = FingerPose::curls([0.5; 5]).to_bones();
        let mut json = serde_json::to_value(ResolvedGrip {
            fingers: bones,
            ..plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one())
        })
        .unwrap();
        for (key, value) in [
            ("curls", serde_json::json!([0.5, 0.5, 0.5, 0.5, 0.5])),
            (
                "trigger_curls",
                serde_json::json!([0.5, 0.5, 0.5, 0.5, 0.5]),
            ),
        ] {
            let mut mixed = json.clone();
            mixed[key] = value;
            assert!(
                serde_json::from_value::<ResolvedGrip>(mixed).is_err(),
                "{key}"
            );
        }
        json.as_object_mut().unwrap().remove("bones");
        assert!(
            serde_json::from_value::<ResolvedGrip>(json).is_err(),
            "no finger pose"
        );
    }

    /// Shipped grip libraries keep decoding as curls and re-encode with the
    /// same fields, so Explorer saves do not churn them.
    #[test]
    fn shipped_grip_libraries_decode_as_curls_unchanged() {
        for name in GRIP_RESOURCES {
            let path = format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"));
            let text = std::fs::read_to_string(&path).unwrap();
            let original: serde_json::Value = serde_json::from_str(&text).unwrap();
            let library: GripLibrary = serde_json::from_str(&text).unwrap();
            let encoded = serde_json::to_value(&library).unwrap();
            for (entry, (before, after)) in library.entries.iter().zip(
                original["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(encoded["entries"].as_array().unwrap()),
            ) {
                assert!(entry.grip.fingers.rest_curls().is_some(), "{name}");
                let keys = |v: &serde_json::Value| {
                    v["grip"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(keys(before), keys(after), "{name} {}", entry.model);
            }
        }
    }

    #[test]
    fn prepared_lookup_uses_model_and_hand_despite_changed_fit_metadata() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let mut library = GripLibrary {
            version: 1,
            solver_revision: SOLVER_REVISION + 1,
            entries: vec![BakedGripEntry {
                authored: false,
                model: "mug".into(),
                hand: "right".into(),
                surface_hash: "old-mesh".into(),
                kinematics_hash: "old-rig".into(),
                hints_hash: "old-hints".into(),
                grip: grip.clone(),
            }],
            ..Default::default()
        };
        assert_eq!(library.lookup("mug", "right"), Some(&grip));
        assert!(library.lookup("mug", "left").is_none());
        assert!(library.lookup("wrench_h", "right").is_none());
        library.entries[0].surface_hash.clear();
        library.entries[0].kinematics_hash.clear();
        library.entries[0].hints_hash.clear();
        assert_eq!(library.lookup("mug", "right"), Some(&grip));
        library.entries[0].grip.item_scale = f32::NAN;
        assert!(library.lookup("mug", "right").is_none());
        library.entries[0].grip = grip;
        library.version = 2;
        assert!(library.lookup("mug", "right").is_none());
    }
    fn closed_cube() -> GripSurface {
        let (vertices, indices) =
            rapier3d::prelude::Cuboid::new(na::Vector3::repeat(0.05)).to_trimesh();
        let triangles = indices
            .iter()
            .map(|indices| {
                indices.map(|i| {
                    let p = vertices[i as usize];
                    point3(p.x, p.y, p.z)
                })
            })
            .collect::<Vec<_>>();
        let surface = GripSurface::new(&triangles).unwrap();
        assert!(surface.closed);
        surface
    }

    #[test]
    fn fingers_starting_inside_a_closed_mesh_cannot_earn_a_valid_grip() {
        let mut hand = straight_fingers();
        hand.palm = vec3(-0.2, 0.0, 0.0);
        let grip = closed_cube().fit_at(&hand, vec3(0.0, 0.0, 0.0), Quaternion::one());
        assert!(
            !grip.is_valid(),
            "moving outward from inside must not count as grasping"
        );
    }

    #[test]
    fn final_finger_segments_cannot_cross_the_mesh_between_clear_endpoints() {
        let mut hand = straight_fingers();
        hand.palm = vec3(-0.2, 0.0, 0.0);
        hand.fingers = std::array::from_fn(|_| {
            vec![vec![vec3(-0.1, 0.0, 0.0), vec3(0.1, 0.0, 0.0)]; CURL_STEPS + 1]
        });
        assert!(!closed_cube().pose_is_clear(
            &hand,
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            &[0.0; 5]
        ));
    }
    #[test]
    fn anchor_region_excludes_corner_candidates_and_changes_bake_fingerprint() {
        let hints = GripHints {
            anchor_region: Some([[-0.01, -0.01, 0.049], [0.01, 0.01, 0.051]]),
            ..Default::default()
        };
        let result = plane(0.05).resolve(&straight_fingers(), &hints).unwrap();
        assert!(result.anchor[0].abs() <= 0.01 && result.anchor[1].abs() <= 0.01);
        assert_ne!(hints.fingerprint(), GripHints::default().fingerprint());
        assert!(
            !GripHints {
                anchor_region: Some([[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]]),
                ..Default::default()
            }
            .is_valid()
        );
    }
    #[test]
    fn upright_axis_selects_direction_and_invalidates_only_opted_in_bakes() {
        let hints = GripHints {
            upright_axis: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        };
        let result = plane(0.05).resolve(&straight_fingers(), &hints).unwrap();
        assert!(result.rotation.rotate_vector(Vector3::unit_x()).y >= 0.8);
        assert_ne!(hints.fingerprint(), GripHints::default().fingerprint());
        assert_eq!(GripHints::default().fingerprint(), "3e42e05263705d8b");
        let small_x = GripHints {
            upright_axis: Some([0.00011, 0.0, 0.0]),
            ..Default::default()
        };
        let small_tilt = GripHints {
            upright_axis: Some([0.00011, 0.000049, 0.0]),
            ..Default::default()
        };
        assert!(small_x.is_valid() && small_tilt.is_valid());
        assert_ne!(small_x.fingerprint(), small_tilt.fingerprint());
        assert_eq!(small_x.fingerprint(), hints.fingerprint());
        assert!(
            !GripHints {
                upright_axis: Some([0.0; 3]),
                ..Default::default()
            }
            .is_valid()
        );
    }
    #[test]
    fn uniform_item_scale_defaults_to_one_and_rejects_invalid_values() {
        let mut grip =
            plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let mut json = serde_json::to_value(&grip).unwrap();
        json.as_object_mut().unwrap().remove("item_scale");
        assert_eq!(
            serde_json::from_value::<ResolvedGrip>(json)
                .unwrap()
                .item_scale,
            1.0
        );
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 11.0] {
            grip.item_scale = scale;
            assert!(!grip.is_valid());
        }
        grip.item_scale = 0.8;
        assert!(grip.is_valid());
    }

    fn turned_grip() -> ResolvedGrip {
        ResolvedGrip {
            item_scale: 0.6,
            offset: vec3(0.03, -0.02, 0.05),
            rotation: (Quaternion::from_angle_y(cgmath::Deg(-70.0))
                * Quaternion::from_angle_x(cgmath::Deg(15.0)))
            .normalize(),
            contacts: [Some([0.1, 0.2, 0.3]), None, None, None, None],
            anchor: [0.4, -0.1, 0.2],
            ..plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one())
        }
    }

    /// `grip`'s item-frame point `q` in hand space.
    fn held_point(grip: &ResolvedGrip, q: Vector3<f32>) -> Vector3<f32> {
        grip.offset + grip.rotation.rotate_vector(q * grip.item_scale)
    }

    fn assert_close(a: Vector3<f32>, b: Vector3<f32>) {
        assert!((a - b).magnitude() < 1e-4, "{a:?} vs {b:?}");
    }

    #[test]
    fn a_mirrored_grip_holds_the_mirror_image_of_the_item() {
        use cgmath::{Matrix3, SquareMatrix, Transform};
        let right = turned_grip();
        let hand = Handedness::Left.mirror();
        // A gun's reflection across its barrel plane, and a melee rig's
        // through a plane off its origin.
        for mirror in [
            crate::vr_config::gun_model_frame("sg_h", Handedness::Left),
            Matrix4::from_translation(vec3(0.2, 0.0, 0.0))
                * hand
                * Matrix4::from_translation(vec3(-0.2, 0.0, 0.0)),
        ] {
            let left = right.mirrored(mirror).unwrap();
            assert!((Matrix3::from(left.rotation).determinant() - 1.0).abs() < 1e-4);
            for q in [
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, -0.5, 0.25),
                vec3(-0.3, 0.7, 0.9),
            ] {
                // Each point of the left-hand item lands where the right
                // hand's lands, reflected across the hand.
                let left_q = mirror.transform_point(Point3::from_vec(q)).to_vec();
                assert_close(
                    held_point(&left, left_q),
                    hand.transform_vector(held_point(&right, q)),
                );
            }
            assert_eq!(left.fingers, right.fingers);
            assert_eq!(left.item_scale, right.item_scale);
            let contact = mirror.transform_point(point3(0.1, 0.2, 0.3));
            assert_eq!(left.contacts[0], Some(contact.into()));
            // And back again.
            let again = left.mirrored(mirror.invert().unwrap()).unwrap();
            assert_close(again.offset, right.offset);
            assert!(again.rotation.dot(right.rotation).abs() > 1.0 - 1e-6);
            assert_close(again.anchor.into(), right.anchor.into());
        }
        // A rotation is no mirror: the result would be a reflection.
        assert!(
            right
                .mirrored(Matrix4::from_angle_y(cgmath::Deg(30.0)))
                .is_none()
        );
    }

    #[test]
    fn a_mirrored_gun_fires_along_its_own_controllers_aim() {
        use cgmath::Transform;
        let mirror = crate::vr_config::gun_model_frame("sg_h", Handedness::Left);
        let aim = vec3(0.0, 0.0, -1.0);
        // The `_h` barrel runs along -X.
        let barrel = vec3(-1.0, 0.0, 0.0);
        let left_barrel = mirror.transform_vector(barrel);
        let fires = |grip: &ResolvedGrip, barrel| grip.rotation.rotate_vector(barrel);
        // Aligned: both hands fire straight down their controller.
        let aligned = ResolvedGrip {
            rotation: Quaternion::from_angle_y(cgmath::Deg(-90.0)),
            ..turned_grip()
        };
        assert_close(fires(&aligned, barrel), aim);
        assert_close(fires(&aligned.mirrored(mirror).unwrap(), left_barrel), aim);
        // Off by some angle: the left is off by as much, mirrored.
        let right = turned_grip();
        let left = right.mirrored(mirror).unwrap();
        assert_close(
            fires(&left, left_barrel),
            Handedness::Left.mirror_point(fires(&right, barrel)),
        );
        assert!(
            (fires(&left, left_barrel).angle(aim).0 - fires(&right, barrel).angle(aim).0).abs()
                < 1e-5
        );
    }

    /// A gun aligned by turning its glove, mirrored, still fires down its own
    /// controller: the glove turn mirrors with the grip and stays a rotation.
    #[test]
    fn a_mirrored_glove_turn_keeps_a_gun_aligned() {
        use cgmath::{Matrix3, SquareMatrix, Transform};
        let mirror = crate::vr_config::gun_model_frame("sg_h", Handedness::Left);
        let barrel = vec3(-1.0, 0.0, 0.0);
        let right = turned_grip();
        let right = ResolvedGrip {
            hand_rotation: crate::weapon_muzzle::aim_turn(right.rotation, barrel, 1.0),
            ..right
        };
        let error = |grip: &ResolvedGrip, barrel| {
            crate::weapon_muzzle::aim_error_deg(grip.hand_rotation * grip.rotation, barrel)
        };
        assert!(error(&right, barrel) < 1e-3);
        let left = right.mirrored(mirror).unwrap();
        assert!((Matrix3::from(left.hand_rotation).determinant() - 1.0).abs() < 1e-4);
        let left_barrel = mirror.transform_vector(barrel);
        assert!(
            error(&left, left_barrel) < 1e-3,
            "{}",
            error(&left, left_barrel)
        );
        // In the controller's frame, the item is the mirror of the right's.
        let in_controller = |grip: &ResolvedGrip, q| grip.hand_rotation * held_point(grip, q);
        let q = vec3(0.3, -0.2, 0.5);
        assert_close(
            in_controller(&left, mirror.transform_point(Point3::from_vec(q)).to_vec()),
            Handedness::Left.mirror_point(in_controller(&right, q)),
        );
    }

    /// The glove turn is saved only when there is one, so every existing
    /// grip file decodes and re-encodes unchanged, and a turned one round-trips.
    #[test]
    fn a_glove_turn_round_trips_and_is_absent_when_none() {
        let grip = turned_grip();
        let plain = serde_json::to_value(&grip).unwrap();
        assert!(plain.get("hand_rotation").is_none(), "{plain}");
        let decoded: ResolvedGrip = serde_json::from_value(plain).unwrap();
        assert!(decoded.hand_rotation.is_one());
        let turned = ResolvedGrip {
            hand_rotation: Quaternion::from_angle_x(cgmath::Deg(12.0)),
            ..grip
        };
        let json = serde_json::to_string(&turned).unwrap();
        assert_eq!(serde_json::from_str::<ResolvedGrip>(&json).unwrap(), turned);
        let mut skewed = serde_json::to_value(&turned).unwrap();
        skewed["hand_rotation"] =
            serde_json::to_value(Quaternion::new(2.0_f32, 0.0, 0.0, 0.0)).unwrap();
        let skewed: ResolvedGrip = serde_json::from_value(skewed).unwrap();
        assert!(!skewed.is_valid(), "a non-unit glove turn is rejected");
    }

    #[test]
    fn shipped_left_weapon_grips_are_their_mirrored_right_ones() {
        let path = format!(
            "{}/../assets/vr-weapon-grips.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let library: GripLibrary =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for entry in library.entries.iter().filter(|e| e.hand == "right") {
            let mirror = crate::vr_weapon_grip::named_model_mirror(&entry.model).unwrap();
            let mirrored = entry.grip.mirrored(mirror).unwrap();
            let left = library.lookup(&entry.model, "left").unwrap();
            assert!(
                (mirrored.offset - left.offset).magnitude() < 1e-4,
                "{}",
                entry.model
            );
            assert!(
                mirrored.rotation.dot(left.rotation).abs() > 1.0 - 1e-6,
                "{}",
                entry.model
            );
        }
    }

    fn entry(model: &str, hand: &str, offset: f32) -> BakedGripEntry {
        BakedGripEntry {
            authored: false,
            model: model.into(),
            hand: hand.into(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip: ResolvedGrip {
                offset: vec3(offset, 0.0, 0.0),
                ..turned_grip()
            },
        }
    }

    #[test]
    fn a_headset_pose_stands_in_mirrored_for_the_other_hand_of_a_mirrored_item() {
        let offset = |selected: Option<(&ResolvedGrip, bool)>| {
            selected.map(|(grip, other)| (grip.offset.x, other))
        };
        let mut library = GripLibrary {
            entries: vec![
                entry("sg_h", "left", 1.0),
                entry("sg_h", "right", 2.0),
                entry("atek_h", "right", 3.0),
            ],
            ..Default::default()
        };
        // Shipped only: each hand its own; a missing hand mirrors the other.
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, true)),
            Some((1.0, false))
        );
        assert_eq!(
            offset(library.select("atek_h", Handedness::Left, true)),
            Some((3.0, true))
        );
        assert_eq!(
            offset(library.select("atek_h", Handedness::Left, false)),
            None
        );
        // A right pose from the headset outranks the shipped left...
        library.prepend(GripLibrary {
            entries: vec![entry("sg_h", "right", 4.0)],
            ..Default::default()
        });
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, true)),
            Some((4.0, true))
        );
        assert_eq!(
            offset(library.select("sg_h", Handedness::Right, true)),
            Some((4.0, false))
        );
        // ...but not for an item drawn alike in both hands.
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, false)),
            Some((1.0, false))
        );
        // A left pose from the headset wins for the left.
        library.prepend(GripLibrary {
            entries: vec![entry("sg_h", "left", 5.0)],
            ..Default::default()
        });
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, true)),
            Some((5.0, false))
        );
        assert_eq!(
            offset(library.select("sg_h", Handedness::Right, true)),
            Some((4.0, false))
        );
        // Unstaged, the left falls back to the headset right, mirrored.
        library.remove_first("sg_h", "left");
        assert_eq!(library.headset, 1);
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, true)),
            Some((4.0, true))
        );
        let mirror = crate::vr_config::gun_model_frame("sg_h", Handedness::Left);
        let (grip, mirrored) = library
            .lookup_for("sg_h", Handedness::Left, Some(mirror))
            .unwrap();
        assert!(mirrored);
        assert_eq!(Some(grip), library.entries[0].grip.mirrored(mirror));
        // One that will not mirror leaves the hand its own.
        let (grip, mirrored) = library
            .lookup_for(
                "sg_h",
                Handedness::Left,
                Some(Matrix4::from_angle_y(cgmath::Deg(30.0))),
            )
            .unwrap();
        assert_eq!((grip.offset.x, mirrored), (1.0, false));
        library.remove_first("sg_h", "right");
        assert_eq!(library.headset, 0);
        assert_eq!(
            offset(library.select("sg_h", Handedness::Left, true)),
            Some((1.0, false))
        );
    }

    fn classes(json: &str) -> GripClasses {
        GripClasses::parse(json).unwrap()
    }

    fn classed_library() -> GripLibrary {
        GripLibrary {
            entries: vec![entry("chemif", "right", 1.0), entry("mug", "right", 2.0)],
            classes: classes(r#"{"version": 1, "classes": [["chemif", "chemif2", "chemif3"]]}"#),
            ..Default::default()
        }
    }

    #[test]
    fn a_class_member_takes_its_canonical_models_grip() {
        let library = classed_library();
        let x = |model| library.lookup(model, "right").map(|g| g.offset.x);
        assert_eq!(x("chemif2"), Some(1.0));
        assert_eq!(x("chemif3"), Some(1.0));
        assert_eq!(
            library
                .lookup_for("chemif2", Handedness::Right, None)
                .map(|(g, _)| g.offset.x),
            Some(1.0)
        );
        // Outside any class, nothing changes.
        assert_eq!(x("mug"), Some(2.0));
        assert_eq!(x("chemil"), None);
        assert_eq!(library.lookup("chemif2", "left"), None);
    }

    #[test]
    fn a_headset_pose_saved_for_a_member_wins_for_the_whole_class() {
        let mut library = classed_library();
        library.prepend(GripLibrary {
            entries: vec![entry("chemif3", "right", 7.0)],
            ..Default::default()
        });
        for model in ["chemif", "chemif2", "chemif3"] {
            assert_eq!(library.lookup(model, "right").unwrap().offset.x, 7.0);
        }
        assert_eq!(library.lookup("mug", "right").unwrap().offset.x, 2.0);
        // One headset entry per class and hand.
        library.replace_headset(entry("chemif2", "right", 8.0));
        assert_eq!(library.headset, 1);
        assert_eq!(library.lookup("chemif", "right").unwrap().offset.x, 8.0);
        library.remove_first("chemif3", "right");
        assert_eq!(library.headset, 0);
        assert_eq!(library.lookup("chemif3", "right").unwrap().offset.x, 1.0);
        // Two members posed for one hand (an older build's file): the later
        // wins, as merge-grips keeps it.
        library.prepend(GripLibrary {
            entries: vec![
                entry("chemif2", "right", 5.0),
                entry("chemif3", "right", 6.0),
            ],
            ..Default::default()
        });
        assert_eq!(library.headset, 1);
        assert_eq!(library.entries[0].model, "chemif");
        assert_eq!(library.lookup("chemif2", "right").unwrap().offset.x, 6.0);
    }

    /// Only the canonical model's entry is the class's grip: a stale entry
    /// under another member (say after reordering the class) never shadows
    /// it.
    #[test]
    fn a_shipped_entry_under_a_non_canonical_member_is_ignored() {
        let mut library = classed_library();
        library.entries.insert(0, entry("chemif2", "right", 9.0));
        assert_eq!(library.lookup("chemif2", "right").unwrap().offset.x, 1.0);
        assert_eq!(library.lookup("chemif", "right").unwrap().offset.x, 1.0);
    }

    #[test]
    fn an_invalid_classes_file_is_rejected() {
        for bad in [
            r#"{"version": 1, "classes": [["a", "b"], ["c", "a"]]}"#,
            r#"{"version": 1, "classes": [["a", "a"]]}"#,
            r#"{"version": 1, "classes": [["a"]]}"#,
            r#"{"version": 1, "classes": [[]]}"#,
            r#"{"version": 2, "classes": [["a", "b"]]}"#,
            r#"{"version": 1, "classes": [["a", "b"]], "extra": 1}"#,
            "not json",
        ] {
            assert!(GripClasses::parse(bad).is_err(), "{bad}");
            assert_eq!(GripClasses::load(Some(bad)).canonical("b"), "b", "{bad}");
        }
        assert_eq!(GripClasses::load(None).canonical("b"), "b");
        let ok = classes(r#"{"version": 1, "classes": [["a", "b", "c"], ["d", "e"]]}"#);
        assert_eq!(ok.canonical("c"), "a");
        assert_eq!(ok.canonical("e"), "d");
        assert_eq!(ok.canonical("f"), "f");
        assert_eq!(
            ok.members("b"),
            Some(&["a", "b", "c"].map(String::from)[..])
        );
        assert_eq!(ok.members("f"), None);
        assert_eq!(
            ok.label("c").unwrap().join(" "),
            "Shared by 3 models (canonical: a)"
        );
        assert_eq!(ok.label("f"), None);
    }

    /// Each class has one shipped grip, its canonical model's: a member's
    /// own entry would never be picked.
    #[test]
    fn shipped_classes_are_valid_and_only_their_canonical_models_have_grips() {
        let path = format!(
            "{}/../assets/{GRIP_CLASSES_FILE}",
            env!("CARGO_MANIFEST_DIR")
        );
        let classes = GripClasses::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        for name in GRIP_RESOURCES {
            let path = format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"));
            let library: GripLibrary =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for entry in &library.entries {
                assert_eq!(
                    classes.canonical(&entry.model),
                    entry.model,
                    "{name}: {} is not its class's canonical model",
                    entry.model
                );
            }
        }
    }

    fn metres(m: f32) -> f32 {
        m / crate::METERS_PER_WORLD_UNIT
    }

    /// Each finger's curl read back off its bones: exact on the open -> fist arc.
    fn curl_of(bones: &FingerBones, finger: usize) -> f32 {
        bones_to_curls(bones)[finger]
    }

    const FINGER_LENGTH: f32 = 0.08;

    /// A rigid 8 cm finger per finger, side by side, pointing +z open and
    /// swinging down to -y as it curls (90 degrees at a fist).
    fn swinging_fingers(bones: &FingerBones) -> [FingerChain; 5] {
        swinging_joints(bones).map(|joints| FingerChain::uniform(&joints, FINGER_RADIUS))
    }

    /// `swinging_fingers`' joints.
    fn swinging_joints(bones: &FingerBones) -> [Vec<Vector3<f32>>; 5] {
        std::array::from_fn(|finger| {
            let angle = curl_of(bones, finger) * std::f32::consts::FRAC_PI_2;
            let along = vec3(0.0, -angle.sin(), angle.cos());
            let pivot = vec3(metres(0.025 * finger as f32), 0.0, 0.0);
            [0.25, 0.5, 0.75, 1.0]
                .map(|s| pivot + along * metres(FINGER_LENGTH * s))
                .to_vec()
        })
    }

    /// A closed box with its top `drop` metres below the fingers' pivots.
    fn slab(drop: f32) -> Vec<[Point3<f32>; 3]> {
        let half = na::Vector3::new(metres(0.1), metres(0.05), metres(0.1));
        let (vertices, indices) = rapier3d::prelude::Cuboid::new(half).to_trimesh();
        let center = vec3(metres(0.05), -metres(drop) - half.y, metres(0.1));
        indices
            .iter()
            .map(|t| {
                t.map(|i| {
                    let p = vertices[i as usize];
                    Point3::from_vec(vec3(p.x, p.y, p.z) + center)
                })
            })
            .collect()
    }

    /// The gap a swinging fingertip at `curl` leaves above `slab(drop)`.
    fn tip_gap_mm(curl: f32, drop: f32) -> f32 {
        let tip = FINGER_LENGTH * (curl * std::f32::consts::FRAC_PI_2).sin();
        (drop - tip - 0.007) * 1000.0
    }

    fn held(fingers: FingerPose) -> ResolvedGrip {
        ResolvedGrip {
            item_scale: 1.0,
            pose_family: "broad".to_owned(),
            offset: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::one(),
            hand_rotation: Quaternion::from_angle_y(cgmath::Deg(5.0)),
            fingers,
            contacts: [None; 5],
            anchor: [0.0; 3],
            score: 0.0,
        }
    }

    fn assert_same_placement(a: &ResolvedGrip, b: &ResolvedGrip) {
        assert_eq!(
            (a.offset, a.rotation, a.hand_rotation, a.item_scale),
            (b.offset, b.rotation, b.hand_rotation, b.item_scale)
        );
    }

    #[test]
    fn tighten_closes_an_open_finger_and_leaves_one_through_the_item_buried() {
        let surface = GripSurface::new(&slab(0.05)).unwrap();
        // Thumb open, index's tip through the slab, the rest open but
        // masked out.
        let grip = ResolvedGrip {
            contacts: [None, Some([1.0, 2.0, 3.0]), None, None, None],
            ..held(FingerPose::curls([0.0, 0.45, 0.0, 0.0, 0.0]).to_bones())
        };
        let fingers = [true, true, false, false, false];
        let out = surface.tighten(&grip, false, fingers, swinging_fingers);
        assert_same_placement(&out.grip, &grip);
        assert!(out.grip.is_valid(), "{:?}", out.grip);
        let FingerPose::Bones { bones, .. } = &out.grip.fingers else {
            panic!("bones stay bones");
        };
        let gap = tip_gap_mm(curl_of(bones, 0), 0.05);
        assert!((0.9..1.2).contains(&gap), "thumb {gap} mm off");
        assert!(matches!(out.fingers[0], FingerFit::Touching { .. }));
        assert!(out.grip.contacts[0].is_some());
        assert!(
            matches!(out.fingers[1], FingerFit::Buried { .. }),
            "{:?}",
            out.fingers[1]
        );
        assert_eq!(
            out.grip.contacts[1],
            Some([1.0, 2.0, 3.0]),
            "its contact is kept"
        );
        let before = grip.fingers.rest_bones();
        assert_eq!(
            bones[FINGER_BONES[1].clone()],
            before[FINGER_BONES[1].clone()]
        );
        for finger in 2..5 {
            assert_eq!(out.fingers[finger], FingerFit::Skipped);
            for i in FINGER_BONES[finger].clone() {
                assert_eq!(bones[i], before[i], "masked finger {finger} unchanged");
            }
        }
    }

    /// Unbury opens a buried finger along its arc until it rests the
    /// clearance off the item, and leaves clear and unchosen fingers be.
    #[test]
    fn unbury_opens_a_buried_finger_to_the_clearance_and_leaves_the_rest() {
        let surface = GripSurface::new(&slab(0.05)).unwrap();
        // Thumb clear; index and middle through the slab, the middle locked.
        let grip = ResolvedGrip {
            contacts: [None, None, Some([1.0, 2.0, 3.0]), None, None],
            ..held(FingerPose::curls([0.0, 0.45, 0.5, 0.0, 0.0]))
        };
        let fingers = [true, true, false, true, true];
        let out = surface.unbury(&grip, false, fingers, swinging_fingers);
        assert_same_placement(&out.grip, &grip);
        let FingerPose::Curls { curls, .. } = out.grip.fingers else {
            panic!("curls stay curls: {:?}", out.grip.fingers);
        };
        assert_eq!(out.fingers[0], FingerUnbury::Clear);
        assert!(
            matches!(out.fingers[1], FingerUnbury::Unburied { amount, .. } if amount < 0.0),
            "{:?}",
            out.fingers[1]
        );
        assert_eq!(out.fingers[2], FingerUnbury::Skipped);
        let gap = tip_gap_mm(curls[1], 0.05);
        assert!((1.0..1.2).contains(&gap), "index {gap} mm off");
        assert_eq!(
            [curls[0], curls[2], curls[3], curls[4]],
            [0.0, 0.5, 0.0, 0.0]
        );
        assert!(out.grip.contacts[1].is_some());
        assert_eq!(
            out.grip.contacts[2],
            Some([1.0, 2.0, 3.0]),
            "the locked finger's contact"
        );
        // Tighten finds it resting there.
        let again = surface.tighten(
            &out.grip,
            false,
            [false, true, false, false, false],
            swinging_fingers,
        );
        assert_eq!(again.fingers[1].outcome(), "snug", "{:?}", again.fingers[1]);
    }

    /// A finger through a thick stock is unburied, then wraps it: each
    /// phalanx seats on it.
    #[test]
    fn unbury_then_tighten_seats_a_finger_through_a_stock_around_it() {
        let (radius, axis) = (0.015, [-0.028, 0.025]);
        let surface = GripSurface::new(&rod(radius, axis)).unwrap();
        let grip = held(FingerPose::curls([0.0, 0.6, 0.0, 0.0, 0.0]));
        let index = [false, true, false, false, false];
        let buried = surface.tighten(&grip, false, index, jointed_fingers);
        assert!(
            matches!(buried.fingers[1], FingerFit::Buried { .. }),
            "{:?}",
            buried.fingers[1]
        );
        let unburied = surface.unbury(&grip, false, index, jointed_fingers);
        assert!(
            matches!(unburied.fingers[1], FingerUnbury::Unburied { .. }),
            "{:?}",
            unburied.fingers[1]
        );
        let out = surface.tighten(&unburied.grip, false, index, jointed_fingers);
        assert!(
            out.fingers[1].outcome().ends_with("+wrap"),
            "{:?}",
            out.fingers[1]
        );
        let gaps = phalanx_gaps_mm(
            &jointed_fingers(&out.grip.fingers.rest_bones())[1],
            radius,
            axis,
        );
        assert!(gaps.iter().all(|gap| (0.9..1.2).contains(gap)), "{gaps:?}");
    }

    /// A finger with only its knuckle through the item is buried too: left
    /// exactly as it was.
    #[test]
    fn a_finger_with_its_knuckle_through_the_item_is_buried() {
        let mut triangles = slab(0.05);
        // A 1 cm bar through every finger's pivot.
        let half = na::Vector3::new(metres(0.1), metres(0.005), metres(0.005));
        let (vertices, indices) = rapier3d::prelude::Cuboid::new(half).to_trimesh();
        triangles.extend(indices.iter().map(|t| {
            t.map(|i| {
                let p = vertices[i as usize];
                Point3::from_vec(vec3(p.x, p.y, p.z) + vec3(metres(0.05), 0.0, 0.0))
            })
        }));
        let surface = GripSurface::new(&triangles).unwrap();
        assert!(surface.closed);
        let from_pivot = |bones: &FingerBones| {
            swinging_joints(bones).map(|mut joints| {
                joints.insert(0, joints[0] - (joints[1] - joints[0]));
                FingerChain::uniform(&joints, FINGER_RADIUS)
            })
        };
        let grip = held(FingerPose::curls([0.0; 5]));
        let out = surface.tighten(&grip, false, [true; 5], from_pivot);
        assert!(
            out.fingers
                .iter()
                .all(|f| matches!(f, FingerFit::Buried { .. })),
            "{:?}",
            out.fingers
        );
        assert_eq!(out.grip.fingers, grip.fingers);
    }

    fn cuboid(center: Vector3<f32>, half: f32) -> Vec<[Point3<f32>; 3]> {
        let (vertices, indices) =
            rapier3d::prelude::Cuboid::new(na::Vector3::repeat(half)).to_trimesh();
        indices
            .iter()
            .map(|t| {
                t.map(|i| {
                    Point3::from_vec(
                        vec3(
                            vertices[i as usize].x,
                            vertices[i as usize].y,
                            vertices[i as usize].z,
                        ) + center,
                    )
                })
            })
            .collect()
    }

    /// A finger deep in a closed part of the item is buried even where the
    /// item overlaps closed shells, or has an open part elsewhere.
    #[test]
    fn a_finger_deep_in_overlapping_or_partly_open_items_is_buried() {
        // Every swinging finger, at any curl, inside a 20 cm box about them.
        let around = cuboid(vec3(metres(0.05), 0.0, 0.0), metres(0.2));
        let overlapping = [
            around.clone(),
            cuboid(vec3(metres(0.1), 0.0, metres(0.05)), metres(0.2)),
        ]
        .concat();
        // A lone open quad far off.
        let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            .map(|[x, y]| point3(metres(1.0 + x * 0.1), metres(y * 0.1), metres(1.0)));
        let partly_open = [
            around,
            vec![[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]],
        ]
        .concat();
        for (name, triangles) in [("overlapping", overlapping), ("partly open", partly_open)] {
            let surface = GripSurface::new(&triangles).unwrap();
            let grip = held(FingerPose::curls([0.3; 5]));
            let out = surface.tighten(&grip, false, [true; 5], swinging_fingers);
            assert!(
                out.fingers
                    .iter()
                    .all(|f| matches!(f, FingerFit::Buried { .. })),
                "{name}: {:?}",
                out.fingers
            );
            assert_eq!(out.grip.fingers, grip.fingers, "{name}");
        }
    }

    /// The mouth of an open cup is not inside it: a point there is only
    /// partly wound about. A closed box's middle is.
    #[test]
    fn the_mouth_of_an_open_cup_is_outside_it() {
        let half = 0.1;
        // A 20 cm box with no top (+y) face.
        let cup: Vec<_> = cuboid(vec3(0.0, 0.0, 0.0), half)
            .into_iter()
            .filter(|t| !t.iter().all(|p| (p.y - half).abs() < 1e-6))
            .collect();
        let mouth = na::Point3::new(0.0, half - 0.02, 0.0);
        assert!(!GripSurface::new(&cup).unwrap().inside(mouth));
        let closed = GripSurface::new(&cuboid(vec3(0.0, 0.0, 0.0), half)).unwrap();
        assert!(closed.inside(mouth));
        assert!(!closed.inside(na::Point3::new(0.0, half + 0.02, 0.0)));
    }

    /// A blade thinner than a fingertip's step at a fixed 48 steps still
    /// stops the finger: steps are as short as the finger's thinnest reach.
    #[test]
    fn a_thin_blade_anywhere_on_a_jointed_fingers_sweep_stops_it() {
        // A thin (0.5 mm) index jointed 2, 2 and 30 cm, each joint turning
        // 120 degrees at a fist: its tip sweeps a whole turn, its chord short.
        let lengths = [0.02, 0.02, 0.3];
        let tip = |curl: f32| {
            let (mut point, mut angle) = (vec3(0.0, 0.0, 0.0), 0.0f32);
            for length in lengths {
                angle += curl * 120f32.to_radians();
                point += vec3(0.0, -angle.sin(), angle.cos()) * metres(length);
            }
            point
        };
        let curling = |bones: &FingerBones| {
            std::array::from_fn(|finger| {
                let mut point = vec3(metres(0.5 * finger as f32), 0.0, 0.0);
                let (mut angle, mut joints) = (0.0f32, vec![point]);
                for (k, length) in lengths.iter().enumerate() {
                    angle += turned(bones, PHALANX_BONES[finger] + k) * 120f32.to_radians();
                    point += vec3(0.0, -angle.sin(), angle.cos()) * metres(*length);
                    joints.push(point);
                }
                FingerChain::uniform(&joints, metres(0.0005))
            })
        };
        // A 1 mm blade across the tip's path at each of many points on it.
        for blade_at in (1..30).map(|i| i as f32 / 31.0) {
            let along = (tip(blade_at + 0.001) - tip(blade_at - 0.001)).normalize();
            let side = vec3(1.0, 0.0, 0.0);
            let (vertices, indices) = rapier3d::prelude::Cuboid::new(na::Vector3::new(
                metres(0.002),
                metres(0.0005),
                metres(0.002),
            ))
            .to_trimesh();
            let up = side.cross(along);
            let center = tip(blade_at) + vec3(metres(0.5), 0.0, 0.0);
            let blade: Vec<_> = indices
                .iter()
                .map(|t| {
                    t.map(|i| {
                        let p = vertices[i as usize];
                        Point3::from_vec(center + side * p.x + along * p.y + up * p.z)
                    })
                })
                .collect();
            let surface = GripSurface::new(&blade).unwrap();
            let grip = held(FingerPose::curls([0.0; 5]));
            let out = surface.tighten(&grip, false, [false, true, false, false, false], curling);
            // Met, never through: the tab is the only thing in reach.
            let chain = &curling(&out.grip.fingers.rest_bones())[1];
            let deepest = surface
                .finger_elements(&out.grip, chain)
                .iter()
                .map(|e| e.gap)
                .fold(f32::INFINITY, f32::min);
            assert!(
                matches!(out.fingers[1], FingerFit::Touching { .. }) && deepest >= 0.0,
                "blade at {blade_at}: {:?}, deepest {deepest}",
                out.fingers[1]
            );
        }
    }

    #[test]
    fn a_thin_blade_between_steps_stops_a_thin_fast_finger() {
        const LENGTH: f32 = 0.3;
        // One long, 0.5 mm thick finger (the index), swinging 90 degrees.
        let long_finger = |bones: &FingerBones| {
            std::array::from_fn(|finger| {
                let angle = curl_of(bones, finger) * std::f32::consts::FRAC_PI_2;
                let along = vec3(0.0, -angle.sin(), angle.cos());
                let pivot = vec3(metres(0.5 * finger as f32), 0.0, 0.0);
                FingerChain::uniform(&[pivot, pivot + along * metres(LENGTH)], metres(0.0005))
            })
        };
        // A 1 mm blade on the index's sweep, half way between 48ths of it.
        let blade_angle = 20.5 / 48.0 * std::f32::consts::FRAC_PI_2;
        let (vertices, indices) = rapier3d::prelude::Cuboid::new(na::Vector3::new(
            metres(0.05),
            metres(0.0005),
            metres(0.05),
        ))
        .to_trimesh();
        let turn = Quaternion::from_angle_x(cgmath::Rad(blade_angle));
        let radial = turn.rotate_vector(vec3(0.0, 0.0, metres(LENGTH)));
        let blade: Vec<_> = indices
            .iter()
            .map(|t| {
                t.map(|i| {
                    let p = vertices[i as usize];
                    Point3::from_vec(
                        vec3(metres(0.5), 0.0, 0.0)
                            + turn.rotate_vector(vec3(p.x, p.y, p.z))
                            + radial,
                    )
                })
            })
            .collect();
        let surface = GripSurface::new(&blade).unwrap();
        let grip = held(FingerPose::curls([0.0; 5]));
        let out = surface.tighten(
            &grip,
            false,
            [false, true, false, false, false],
            long_finger,
        );
        assert!(
            matches!(out.fingers[1], FingerFit::Touching { .. }),
            "{:?}",
            out.fingers[1]
        );
        let curl = out.grip.fingers.rest_curls().unwrap()[1];
        assert!(curl * std::f32::consts::FRAC_PI_2 < blade_angle, "{curl}");
    }

    #[test]
    fn tighten_leaves_a_finger_with_nothing_in_reach_and_keeps_curls_curls() {
        let surface = GripSurface::new(&slab(0.5)).unwrap();
        let grip = held(FingerPose::curls([0.2; 5]));
        let out = surface.tighten(&grip, false, [true; 5], swinging_fingers);
        assert_eq!(out.fingers, [FingerFit::NoContact; 5]);
        assert_eq!(out.grip.fingers, grip.fingers);
        // A curl grip tightens its curls, onto the bones the bone grip gets.
        let surface = GripSurface::new(&slab(0.05)).unwrap();
        let curls = surface.tighten(&grip, false, [true; 5], swinging_fingers);
        let bones = surface.tighten(
            &held(grip.fingers.to_bones()),
            false,
            [true; 5],
            swinging_fingers,
        );
        let FingerPose::Curls { curls: tight, .. } = curls.grip.fingers else {
            panic!("curls stay curls");
        };
        assert!(
            valid_finger_curls(&tight) && tight.iter().all(|c| *c > 0.3),
            "{tight:?}"
        );
        assert_bones_eq(&curls_to_bones(tight), &bones.grip.fingers.rest_bones());
    }

    /// A rest index closed past the trigger pose's would open the finger as
    /// the trigger is pulled: the pulled index comes along to the rest's.
    #[test]
    fn a_rest_index_closing_past_the_pulled_one_takes_it_along() {
        let surface = GripSurface::new(&slab(0.05)).unwrap();
        let grip = held(FingerPose::curls([0.0; 5]).with_trigger_index(0.1));
        let out = surface.tighten(&grip, false, [true; 5], swinging_fingers);
        let FingerPose::Curls {
            curls,
            trigger_curls: Some(pulled),
        } = out.grip.fingers
        else {
            panic!("{:?}", out.grip.fingers);
        };
        assert!(curls[1] > 0.3, "{curls:?}");
        assert_eq!(pulled, curls, "the pulled pose is the rest");
        let bones = surface.tighten(
            &held(grip.fingers.to_bones()),
            false,
            [true; 5],
            swinging_fingers,
        );
        let FingerPose::Bones {
            bones,
            trigger_bones: Some(pulled),
        } = bones.grip.fingers
        else {
            unreachable!()
        };
        assert_eq!(pulled, bones);
    }

    /// A thin edge (a trigger) between two samples, within the finger's
    /// radius of the segment joining them, touches the finger.
    #[test]
    fn a_thin_edge_between_samples_touches_the_finger() {
        let half = na::Vector3::repeat(metres(0.001));
        let (vertices, indices) = rapier3d::prelude::Cuboid::new(half).to_trimesh();
        let center = vec3(0.0, -metres(0.004), metres(0.01));
        let edge: Vec<_> = indices
            .iter()
            .map(|t| {
                t.map(|i| {
                    Point3::from_vec(
                        vec3(
                            vertices[i as usize].x,
                            vertices[i as usize].y,
                            vertices[i as usize].z,
                        ) + center,
                    )
                })
            })
            .collect();
        let surface = GripSurface::new(&edge).unwrap();
        let finger = FingerChain::uniform(
            &[vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, metres(0.02))],
            FINGER_RADIUS,
        );
        let elements = surface.finger_elements(&held(FingerPose::curls([0.0; 5])), &finger);
        let nearest = elements.iter().map(|e| e.gap).fold(f32::INFINITY, f32::min);
        // 3 mm off the finger's axis: 4 mm into its 7 mm radius.
        assert!(
            (nearest - metres(-0.004)).abs() < metres(0.0005),
            "{} mm",
            nearest * crate::METERS_PER_WORLD_UNIT * 1000.0
        );
    }

    #[test]
    fn tighten_measures_the_drawn_item_at_its_scale() {
        // Half-size geometry drawn at twice the size is the same item.
        let half: Vec<_> = slab(0.05)
            .iter()
            .map(|t| t.map(|p| Point3::from_vec(p.to_vec() * 0.5)))
            .collect();
        let grip = held(FingerPose::curls([0.0; 5]));
        let scaled = ResolvedGrip {
            item_scale: 2.0,
            ..grip.clone()
        };
        let a = GripSurface::new(&slab(0.05)).unwrap().tighten(
            &grip,
            false,
            [true; 5],
            swinging_fingers,
        );
        let b =
            GripSurface::new(&half)
                .unwrap()
                .tighten(&scaled, false, [true; 5], swinging_fingers);
        let (FingerPose::Curls { curls: a, .. }, FingerPose::Curls { curls: b, .. }) =
            (a.grip.fingers, b.grip.fingers)
        else {
            unreachable!()
        };
        assert!(
            a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3),
            "{a:?} {b:?}"
        );
    }

    #[test]
    fn tightening_the_trigger_pose_closes_only_its_index_and_keeps_the_rest() {
        let surface = GripSurface::new(&slab(0.05)).unwrap();
        let rest = FingerPose::curls([0.1, 0.0, 0.2, 0.2, 0.2]).to_bones();
        let grip = held(rest.clone());
        let out = surface.tighten(
            &grip,
            true,
            [false, true, false, false, false],
            swinging_fingers,
        );
        let FingerPose::Bones {
            bones,
            trigger_bones: Some(pulled),
        } = &out.grip.fingers
        else {
            panic!("a trigger pose: {:?}", out.grip.fingers);
        };
        assert_eq!(*bones, rest.rest_bones(), "the rest is untouched");
        assert_eq!(*pulled, with_index(bones, pulled), "only the index moves");
        let gap = tip_gap_mm(curl_of(pulled, 1), 0.05);
        assert!((0.9..1.2).contains(&gap), "{gap} mm");
        assert_eq!(
            out.grip.contacts, grip.contacts,
            "contacts describe the rest"
        );
        // A rest tighten carries the moved fingers into the trigger pose but
        // keeps its pulled index.
        let again = surface.tighten(&out.grip, false, [true; 5], swinging_fingers);
        let FingerPose::Bones {
            bones: rest,
            trigger_bones: Some(again_pulled),
        } = &again.grip.fingers
        else {
            unreachable!()
        };
        assert_eq!(*again_pulled, with_index(rest, pulled));
    }

    /// How far `bones[bone]` has turned from open toward the fist (0..=1).
    fn turned(bones: &FingerBones, bone: usize) -> f32 {
        let (open, fist) = (
            open_right_hand().finger_bones(),
            fist_right_hand().finger_bones(),
        );
        rotation_error(open[bone], bones[bone]) / rotation_error(open[bone], fist[bone])
    }

    /// Jointed fingers side by side in the yz plane, pointing +z open:
    /// phalanges 4.5, 2.8 and 2.2 cm (the thumb the last two), each joint
    /// bending down toward -y, 70 degrees at a fist, as far as its bone has
    /// turned.
    fn jointed_fingers(bones: &FingerBones) -> [FingerChain; 5] {
        std::array::from_fn(|finger| {
            let lengths: &[f32] = match finger {
                0 => &[0.028, 0.022],
                _ => &[0.045, 0.028, 0.022],
            };
            let mut point = vec3(metres(0.025 * finger as f32), 0.0, 0.0);
            let (mut angle, mut joints) = (0.0f32, vec![point]);
            for (k, length) in lengths.iter().enumerate() {
                angle += turned(bones, PHALANX_BONES[finger] + k) * 70f32.to_radians();
                point += vec3(0.0, -angle.sin(), angle.cos()) * metres(*length);
                joints.push(point);
            }
            FingerChain::uniform(&joints, FINGER_RADIUS)
        })
    }

    /// A closed cylinder along x through (y, z) = `axis` (metres).
    fn rod(radius: f32, axis: [f32; 2]) -> Vec<[Point3<f32>; 3]> {
        let (vertices, indices) =
            rapier3d::prelude::Cylinder::new(metres(0.2), metres(radius)).to_trimesh(96);
        let center = vec3(0.0, metres(axis[0]), metres(axis[1]));
        indices
            .iter()
            .map(|t| {
                // Its y axis laid along x.
                t.map(|i| {
                    let p = vertices[i as usize];
                    Point3::from_vec(vec3(p.y, -p.x, p.z) + center)
                })
            })
            .collect()
    }

    /// Each phalanx's gap (mm) past `FINGER_RADIUS` to `rod(radius, axis)`.
    fn phalanx_gaps_mm(chain: &FingerChain, radius: f32, axis: [f32; 2]) -> Vec<f32> {
        let axis = cgmath::vec2(metres(axis[0]), metres(axis[1]));
        let yz = |p: Vector3<f32>| cgmath::vec2(p.y, p.z);
        chain
            .phalanges
            .iter()
            .map(|spheres| {
                let (a, b) = (
                    yz(spheres[0].center),
                    yz(spheres[PHALANX_SAMPLES - 1].center),
                );
                let t = ((axis - a).dot(b - a) / (b - a).magnitude2()).clamp(0.0, 1.0);
                let distance = (a + (b - a) * t - axis).magnitude();
                (distance - metres(radius) - FINGER_RADIUS) * crate::METERS_PER_WORLD_UNIT * 1000.0
            })
            .collect()
    }

    /// An index whose base phalanx meets a rod first wraps it: its middle
    /// and then its last phalanx curl on until each touches too.
    #[test]
    fn tighten_wraps_a_finger_around_what_its_base_touches() {
        let (radius, axis) = (0.015, [-0.028, 0.025]);
        let surface = GripSurface::new(&rod(radius, axis)).unwrap();
        assert!(surface.closed);
        let grip = held(FingerPose::curls([0.0; 5]));
        let fingers = [false, true, false, false, false];
        let out = surface.tighten(&grip, false, fingers, jointed_fingers);
        assert_same_placement(&out.grip, &grip);
        assert!(out.grip.is_valid(), "{:?}", out.grip);
        assert_eq!(
            out.fingers[1].outcome(),
            "closed+wrap",
            "{:?}",
            out.fingers[1]
        );
        let FingerPose::Bones { bones, .. } = &out.grip.fingers else {
            panic!(
                "a wrapped finger leaves its curl arc: {:?}",
                out.grip.fingers
            );
        };
        // Base, middle and tip each seat on the rod in turn.
        let gaps = phalanx_gaps_mm(&jointed_fingers(bones)[1], radius, axis);
        assert!(gaps.iter().all(|gap| (0.9..1.2).contains(gap)), "{gaps:?}");
        // The joints past the base curled on beyond it.
        let index = FINGER_BONES[1].start;
        let curled = [1, 2, 3].map(|k| turned(bones, index + k));
        assert!(curled[0] < curled[1] && curled[0] < curled[2], "{curled:?}");
        // Again: everything already rests on the rod, nothing moves.
        let again = surface.tighten(&out.grip, false, fingers, jointed_fingers);
        assert_eq!(again.fingers[1].outcome(), "snug", "{:?}", again.fingers[1]);
        assert_eq!(again.grip.fingers, out.grip.fingers);
        let before = grip.fingers.rest_bones();
        for f in [0, 2, 3, 4] {
            assert_eq!(out.fingers[f], FingerFit::Skipped);
            assert_eq!(
                bones[FINGER_BONES[f].clone()],
                before[FINGER_BONES[f].clone()]
            );
        }
    }

    /// An index that meets a trigger with its tip moves as it always did:
    /// the whole finger along its curl, nothing left to wrap.
    #[test]
    fn a_finger_touching_at_its_tip_does_not_wrap() {
        // A 2 mm edge under where the open index's tip ends up.
        let edge = cuboid(
            vec3(metres(0.025), -metres(0.03), metres(0.08)),
            metres(0.001),
        );
        let surface = GripSurface::new(&edge).unwrap();
        let grip = held(FingerPose::curls([0.1, 0.0, 0.2, 0.2, 0.2]));
        let out = surface.tighten(
            &grip,
            true,
            [false, true, false, false, false],
            jointed_fingers,
        );
        assert_eq!(out.fingers[1].outcome(), "closed", "{:?}", out.fingers[1]);
        let FingerPose::Curls {
            curls,
            trigger_curls: Some(pulled),
        } = out.grip.fingers
        else {
            panic!("curls stay curls: {:?}", out.grip.fingers);
        };
        assert_eq!(curls, [0.1, 0.0, 0.2, 0.2, 0.2], "the rest is kept");
        assert_eq!(
            [pulled[0], pulled[2], pulled[3], pulled[4]],
            [0.1, 0.2, 0.2, 0.2]
        );
        let FingerFit::Touching { gap, .. } = out.fingers[1] else {
            unreachable!()
        };
        let gap = gap * crate::METERS_PER_WORLD_UNIT * 1000.0;
        assert!((0.9..1.2).contains(&gap), "{gap} mm");
        let chain = &jointed_fingers(&out.grip.fingers.at(1.0).rest_bones())[1];
        let nearest = surface
            .finger_elements(&out.grip, chain)
            .into_iter()
            .min_by(|a, b| a.gap.total_cmp(&b.gap))
            .unwrap();
        assert_eq!(nearest.phalanx, 2, "the tip touches");
    }

    /// Through the item at every curl it may take, a finger is buried: left
    /// as it was, and said so.
    #[test]
    fn a_finger_through_the_item_at_every_curl_is_buried_and_kept() {
        // A box around every finger at any curl.
        let block = cuboid(vec3(0.0, 0.0, 0.0), metres(0.3));
        let surface = GripSurface::new(&block).unwrap();
        let grip = held(FingerPose::curls([0.3; 5]).to_bones());
        let out = surface.tighten(&grip, false, [true; 5], jointed_fingers);
        assert!(
            out.fingers
                .iter()
                .all(|f| matches!(f, FingerFit::Buried { .. })),
            "{:?}",
            out.fingers
        );
        assert_eq!(out.fingers[0].outcome(), "buried");
        assert_eq!(out.grip.fingers, grip.fingers);
    }
}
