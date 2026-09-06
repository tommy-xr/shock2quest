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

use crate::{hand_pose::FingerAmounts, vr_config::Handedness};

pub const CURL_STEPS: usize = 24;
/// Bump when solver policy changes require existing results to be rebaked.
pub const SOLVER_REVISION: u32 = 2;
const FINGER_RADIUS: f32 = 0.007 / crate::METERS_PER_WORLD_UNIT;

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
    pub curls: [f32; 5],
    /// Contact samples in item-local space; null means no contact was found.
    pub contacts: [Option<[f32; 3]>; 5],
    pub anchor: [f32; 3],
    pub score: f32,
}

fn default_item_scale() -> f32 {
    1.0
}

impl ResolvedGrip {
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
            && (self.rotation.magnitude2() - 1.0).abs() < 0.001
            && self
                .curls
                .iter()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
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

    pub fn finger_amounts(&self) -> FingerAmounts {
        let [thumb, index, middle, ring, pinky] = self.curls;
        FingerAmounts {
            thumb,
            index,
            middle,
            ring,
            pinky,
        }
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
        let volume: f32 = triangles
            .iter()
            .map(|t| {
                let a = vec3(t[0].x, t[0].y, t[0].z);
                let b = vec3(t[1].x, t[1].y, t[1].z);
                let c = vec3(t[2].x, t[2].y, t[2].z);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
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
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let mut outward = vec3(0.0, 0.0, 0.0);
                outward[axis] = sign;
                for u in [-0.7, 0.0, 0.7] {
                    for v in [-0.7, 0.0, 0.7] {
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
                                        let curls = &resolved.curls[1..];
                                        let min = curls.iter().copied().fold(1.0, f32::min);
                                        let max = curls.iter().copied().fold(0.0, f32::max);
                                        resolved.score -= (max - min) * 0.5;
                                    }
                                    "trigger" => resolved.score -= resolved.curls[1] * 0.5,
                                    _ => {
                                        resolved.score -= resolved.curls.iter().sum::<f32>() * 0.05
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
                result.curls[finger] = *amount;
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
            && self.pose_is_clear(hand, result.offset, result.rotation, &result.curls))
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
                candidate.score +=
                    (candidate.curls[2] + candidate.curls[3] + candidate.curls[4]) * 0.7;
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
            curls,
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
    pub fn lookup(
        &self,
        model: &str,
        hand: &str,
        surface_hash: &str,
        kinematics_hash: &str,
        hints_hash: &str,
    ) -> Option<&ResolvedGrip> {
        if self.version != 1 || self.solver_revision != SOLVER_REVISION {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| {
                entry.model == model
                    && entry.hand == hand
                    && entry.surface_hash == surface_hash
                    && entry.kinematics_hash == kinematics_hash
                    && entry.hints_hash == hints_hash
                    && entry.grip.is_valid()
            })
            .map(|entry| &entry.grip)
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
        assert!(grip.curls.iter().all(|c| (*c - 8.0 / 24.0).abs() < 1e-6));
        assert!(
            grip.contacts
                .iter()
                .all(|p| (p.unwrap()[2] - 0.05).abs() < 1e-6)
        );
    }

    #[test]
    fn unreachable_fingers_use_nearest_pose_instead_of_closing_into_empty_space() {
        let grip = plane(-0.5).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        assert_eq!(grip.curls, [0.0; 5]);
        assert_eq!(grip.contacts, [None; 5]);
    }

    #[test]
    fn changing_item_origin_does_not_change_finger_contact() {
        let hand = straight_fingers();
        let a = plane(0.05).fit_at(&hand, vec3(0.0, 0.0, 0.0), Quaternion::one());
        let b = plane(1.05).fit_at(&hand, vec3(0.0, 0.0, -1.0), Quaternion::one());
        assert_eq!(a.curls, b.curls);
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
        assert_eq!(restored.curls, grip.curls);
        restored.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        assert!(!restored.is_valid());
    }
    #[test]
    fn prepared_lookup_rejects_wrong_hand_stale_mesh_and_stale_rig() {
        let grip = plane(0.05).fit_at(&straight_fingers(), vec3(0.0, 0.0, 0.0), Quaternion::one());
        let mut library = GripLibrary {
            version: 1,
            solver_revision: SOLVER_REVISION,
            entries: vec![BakedGripEntry {
                authored: false,
                model: "mug".into(),
                hand: "right".into(),
                surface_hash: "mesh1".into(),
                kinematics_hash: "rig1".into(),
                hints_hash: "hints1".into(),
                grip,
            }],
        };
        assert!(
            library
                .lookup("mug", "right", "mesh1", "rig1", "hints1")
                .is_some()
        );
        assert!(
            library
                .lookup("mug", "left", "mesh1", "rig1", "hints1")
                .is_none()
        );
        assert!(
            library
                .lookup("mug", "right", "mesh2", "rig1", "hints1")
                .is_none()
        );
        assert!(
            library
                .lookup("mug", "right", "mesh1", "rig2", "hints1")
                .is_none()
        );
        assert!(
            library
                .lookup("mug", "right", "mesh1", "rig1", "hints2")
                .is_none()
        );
        library.solver_revision = SOLVER_REVISION + 1;
        assert!(
            library
                .lookup("mug", "right", "mesh1", "rig1", "hints1")
                .is_none()
        );
        library.solver_revision = SOLVER_REVISION;
        library.version = 2;
        assert!(
            library
                .lookup("mug", "right", "mesh1", "rig1", "hints1")
                .is_none()
        );
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
