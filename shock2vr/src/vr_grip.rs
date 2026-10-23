//! Deterministic pickup fitting against visible triangles. All outputs are
//! hand-local; tracked motion never re-runs the search or changes item scale.
use cgmath::{
    EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Rotation3, Vector3, vec3,
};
use rapier3d::{
    na,
    parry::query::{PointQuery, Ray, RayCast},
    prelude::{TriMesh, TriMeshFlags},
};
use serde::{Deserialize, Serialize};

use crate::{
    hand_pose::{FingerAmounts, FingerBones, HandShape, Pose, fist_right_hand, open_right_hand},
    vr_config::Handedness,
};

pub const CURL_STEPS: usize = 24;
/// Bump when solver policy changes require existing results to be rebaked.
pub const SOLVER_REVISION: u32 = 3;
const FINGER_RADIUS: f32 = 0.007 / crate::METERS_PER_WORLD_UNIT;

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
    /// Item origin and rotation in tracked-hand space (world units).
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
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
            && self.fingers.is_valid()
            && self
                .anchor
                .iter()
                .chain(self.contacts.iter().flatten().flatten())
                .all(|c| c.is_finite())
    }

    /// Bounds of the visible fitted item in controller-local space, for capture framing.
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
            fingers: FingerPose::curls(curls),
            contacts,
            anchor: [0.0; 3],
            score,
        }
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
}

impl GripLibrary {
    /// Prepared poses are selected by model identity and hand. Fit fingerprints
    /// and solver revisions are provenance, never runtime eligibility gates.
    pub fn lookup(&self, model: &str, hand: &str) -> Option<&ResolvedGrip> {
        if self.version != 1 {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.model == model && entry.hand == hand && entry.grip.is_valid())
            .map(|entry| &entry.grip)
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

impl GripLibrary {
    /// Put `overrides` ahead of every entry; `lookup` takes the first match.
    pub fn prepend(&mut self, overrides: GripLibrary) {
        self.entries.splice(0..0, overrides.entries);
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
}

impl GripOverrides {
    /// A missing file is normal; an unreadable one is logged and ignored.
    /// Logs to stdout: the Quest runtime has no tracing subscriber.
    pub fn load(path: &std::path::Path) -> Option<GripOverrides> {
        let text = std::fs::read_to_string(path).ok()?;
        let path = path.display();
        match serde_json::from_str::<GripOverrides>(&text) {
            Ok(overrides) if overrides.grips.version == 1 => {
                let entries = overrides.grips.entries.len();
                let support = overrides.support.len();
                println!(
                    "SHOCK2QUEST_VR_GRIP_OVERRIDES path={path} entries={entries} support={support}"
                );
                Some(overrides)
            }
            Ok(overrides) => {
                let version = overrides.grips.version;
                println!("SHOCK2QUEST_VR_GRIP_OVERRIDES path={path} ignored=version_{version}");
                None
            }
            Err(error) => {
                println!("SHOCK2QUEST_VR_GRIP_OVERRIDES path={path} ignored=\"{error}\"");
                None
            }
        }
    }
}

impl Default for GripLibrary {
    fn default() -> Self {
        Self {
            version: 1,
            solver_revision: SOLVER_REVISION,
            entries: Vec::new(),
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
}
