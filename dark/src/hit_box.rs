//! Per-joint collision shapes for creature limbs.
//!
//! Both the damage hitboxes (`HitBoxManager`) and the physics ragdoll
//! (`RagDollManager`) need a per-joint collision volume. Historically both used
//! the axis-aligned bounding box of each joint's skinned vertices, but those
//! cluster around the joint origin (verts are joint-local) and leave the *bone
//! segments between joints* uncovered (the `hitbox_analyzer` coverage metric
//! showed limbs at ~10-50%).
//!
//! `fit_hit_box_shapes` produces a better-fitting shape per joint, in
//! **joint-local space**: for a joint with a single child it returns a capsule
//! spanning from the joint toward that child (covering the bone), sized to
//! enclose the joint's verts; for leaf/branching joints it falls back to the
//! vertex AABB. This is the single source of truth consumed by the analyzer and
//! both runtime managers.

use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Vector3, Vector4};

use crate::motion::JointId;
use crate::ss2_bin_ai_loader::SystemShock2AIMesh;
use crate::ss2_skeleton::Skeleton;

/// Minimum capsule radius / box half-extent so degenerate (single-vertex) joints
/// still produce a valid, non-zero shape.
const MIN_EXTENT: f32 = 0.04;

/// A per-joint collision shape, expressed in the joint's local frame.
#[derive(Clone, Debug)]
pub enum HitBoxShape {
    /// Axis-aligned box: half-extents and center, in joint-local space.
    Cuboid {
        half_extents: Vector3<f32>,
        center: Vector3<f32>,
    },
    /// Capsule between two joint-local points `a` and `b` with the given radius.
    Capsule {
        a: Vector3<f32>,
        b: Vector3<f32>,
        radius: f32,
    },
}

/// Fit a collision shape per joint (joint-local space). Joints with a single
/// child become a capsule spanning the bone toward that child; others a box.
pub fn fit_hit_box_shapes(
    mesh: &SystemShock2AIMesh,
    skeleton: &Skeleton,
) -> HashMap<JointId, HitBoxShape> {
    let joint_verts = mesh.joint_vertex_positions();
    let world = skeleton.world_transforms();

    // Map each joint to its child joints.
    let mut children: HashMap<JointId, Vec<JointId>> = HashMap::new();
    for bone in skeleton.bones() {
        if let Some(parent) = bone.parent_id {
            children.entry(parent).or_default().push(bone.joint_id);
        }
    }

    let mut out = HashMap::new();
    for (joint, verts) in &joint_verts {
        if verts.is_empty() {
            continue;
        }

        // Capsule toward the single child (covers the bone) when possible.
        let single_child = children
            .get(joint)
            .and_then(|c| if c.len() == 1 { Some(c[0]) } else { None });

        let shape = match single_child {
            Some(child) if (*joint as usize) < world.len() && (child as usize) < world.len() => {
                let child_local = child_pos_in_joint_frame(&world, *joint, child);
                let a = Vector3::new(0.0, 0.0, 0.0);
                if (child_local - a).magnitude() < 1e-3 {
                    cuboid_from_verts(verts)
                } else {
                    let radius = verts
                        .iter()
                        .map(|v| point_segment_distance(point_to_vec(*v), a, child_local))
                        .fold(0.0f32, f32::max)
                        .max(MIN_EXTENT);
                    HitBoxShape::Capsule {
                        a,
                        b: child_local,
                        radius,
                    }
                }
            }
            _ => cuboid_from_verts(verts),
        };
        out.insert(*joint, shape);
    }
    out
}

fn child_pos_in_joint_frame(
    world: &[Matrix4<f32>; 40],
    joint: JointId,
    child: JointId,
) -> Vector3<f32> {
    let inv = world[joint as usize]
        .invert()
        .unwrap_or_else(Matrix4::identity);
    let cw = world[child as usize].w; // child world translation (4th column)
    let c = inv * Vector4::new(cw.x, cw.y, cw.z, 1.0);
    Vector3::new(c.x, c.y, c.z)
}

fn cuboid_from_verts(verts: &[Point3<f32>]) -> HitBoxShape {
    let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for v in verts {
        min.x = min.x.min(v.x);
        min.y = min.y.min(v.y);
        min.z = min.z.min(v.z);
        max.x = max.x.max(v.x);
        max.y = max.y.max(v.y);
        max.z = max.z.max(v.z);
    }
    let center = (min + max) * 0.5;
    let half_extents = Vector3::new(
        ((max.x - min.x) * 0.5).max(MIN_EXTENT),
        ((max.y - min.y) * 0.5).max(MIN_EXTENT),
        ((max.z - min.z) * 0.5).max(MIN_EXTENT),
    );
    HitBoxShape::Cuboid {
        half_extents,
        center,
    }
}

fn point_to_vec(p: Point3<f32>) -> Vector3<f32> {
    Vector3::new(p.x, p.y, p.z)
}

/// Distance from point `p` to segment `a`-`b`.
fn point_segment_distance(p: Vector3<f32>, a: Vector3<f32>, b: Vector3<f32>) -> f32 {
    let ab = b - a;
    let len2 = ab.magnitude2();
    if len2 < 1e-9 {
        return (p - a).magnitude();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).magnitude()
}
