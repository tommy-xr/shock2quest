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
use engine::scene::{SceneObject, VertexPosition, color_material, lines_mesh};

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

// ---------------------------------------------------------------------------
// Debug rendering
// ---------------------------------------------------------------------------

/// Number of segments per ring/arc in the capsule wireframe.
const WIRE_SEGMENTS: usize = 12;

/// Build a line-list wireframe of the per-joint fitted shapes, each transformed
/// by its joint's world matrix (`world_joints[joint_id]`). Mirrors
/// `Skeleton::debug_draw`: a single `SceneObject` of colored lines that overlays
/// the animated mesh. Shared by `dark_viewer --debug-hitboxes` and the
/// `debug_hitbox` scene so both render exactly what `fit_hit_box_shapes`
/// produced (no physics round-trip), which separates a fit/mapping bug from a
/// ragdoll-conversion bug.
pub fn draw_debug_hit_box_shapes(
    shapes: &HashMap<JointId, HitBoxShape>,
    world_joints: &[Matrix4<f32>],
    color: Vector3<f32>,
) -> Vec<SceneObject> {
    let mut verts: Vec<VertexPosition> = Vec::new();

    for (joint, shape) in shapes {
        let idx = *joint as usize;
        if idx >= world_joints.len() {
            continue;
        }
        let xform = world_joints[idx];
        match shape {
            HitBoxShape::Cuboid {
                half_extents,
                center,
            } => append_box_lines(&mut verts, &xform, *center, *half_extents),
            HitBoxShape::Capsule { a, b, radius } => {
                append_capsule_lines(&mut verts, &xform, *a, *b, *radius)
            }
        }
    }

    if verts.is_empty() {
        return Vec::new();
    }

    vec![SceneObject::new(
        color_material::create(color),
        Box::new(lines_mesh::create(verts)),
    )]
}

/// A world-space wire sphere - three orthogonal great circles - as one
/// `SceneObject` of colored lines. For overlays that mark a spherical volume
/// (a gesture zone, a trigger radius) rather than a fitted body part.
pub fn draw_debug_wire_sphere(
    center: Vector3<f32>,
    radius: f32,
    color: Vector3<f32>,
) -> SceneObject {
    SceneObject::new(
        color_material::create(color),
        Box::new(lines_mesh::create(wire_sphere_segments(center, radius))),
    )
}

/// The line segments of [`draw_debug_wire_sphere`], two vertices each: three
/// closed rings of `WIRE_SEGMENTS` around X, Y and Z.
fn wire_sphere_segments(center: Vector3<f32>, radius: f32) -> Vec<VertexPosition> {
    let identity = Matrix4::identity();
    let mut verts: Vec<VertexPosition> = Vec::new();
    let axes = [
        (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
    ];
    for (u, v) in axes {
        push_ring(&mut verts, &identity, &ring_points(center, u, v, radius));
    }
    verts
}

/// `WIRE_SEGMENTS` points around a circle of `radius` at `center`, in the
/// plane spanned by the unit vectors `u` and `v`.
fn ring_points(
    center: Vector3<f32>,
    u: Vector3<f32>,
    v: Vector3<f32>,
    radius: f32,
) -> Vec<Vector3<f32>> {
    (0..WIRE_SEGMENTS)
        .map(|k| {
            let t = (k as f32) / (WIRE_SEGMENTS as f32) * std::f32::consts::TAU;
            center + (u * t.cos() + v * t.sin()) * radius
        })
        .collect()
}

/// The closed loop of segments through `ring`'s points, transformed by `xform`.
fn push_ring(verts: &mut Vec<VertexPosition>, xform: &Matrix4<f32>, ring: &[Vector3<f32>]) {
    for k in 0..ring.len() {
        push_segment(verts, xform, ring[k], ring[(k + 1) % ring.len()]);
    }
}

/// Transform a joint-local point to world space (homogeneous, w=1).
fn xform_point(xform: &Matrix4<f32>, p: Vector3<f32>) -> Vector3<f32> {
    let c = xform * Vector4::new(p.x, p.y, p.z, 1.0);
    Vector3::new(c.x, c.y, c.z)
}

fn push_segment(
    verts: &mut Vec<VertexPosition>,
    xform: &Matrix4<f32>,
    a: Vector3<f32>,
    b: Vector3<f32>,
) {
    verts.push(VertexPosition {
        position: xform_point(xform, a),
    });
    verts.push(VertexPosition {
        position: xform_point(xform, b),
    });
}

/// 12 edges of an axis-aligned box (joint-local), transformed by `xform`.
fn append_box_lines(
    verts: &mut Vec<VertexPosition>,
    xform: &Matrix4<f32>,
    center: Vector3<f32>,
    he: Vector3<f32>,
) {
    // 8 corners.
    let mut corner = [Vector3::new(0.0, 0.0, 0.0); 8];
    let mut i = 0;
    for sx in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 1.0] {
                corner[i] = center + Vector3::new(sx * he.x, sy * he.y, sz * he.z);
                i += 1;
            }
        }
    }
    // Index layout: bit2=x, bit1=y, bit0=z.
    let edges = [
        (0, 1),
        (0, 2),
        (0, 4),
        (1, 3),
        (1, 5),
        (2, 3),
        (2, 6),
        (3, 7),
        (4, 5),
        (4, 6),
        (5, 7),
        (6, 7),
    ];
    for (s, e) in edges {
        push_segment(verts, xform, corner[s], corner[e]);
    }
}

/// Capsule wireframe (joint-local) between `a` and `b`: end rings, longitudinal
/// lines, and two great-circle arcs over each hemispherical cap.
fn append_capsule_lines(
    verts: &mut Vec<VertexPosition>,
    xform: &Matrix4<f32>,
    a: Vector3<f32>,
    b: Vector3<f32>,
    radius: f32,
) {
    let axis_vec = b - a;
    let len = axis_vec.magnitude();
    if len < 1e-5 {
        return;
    }
    let axis = axis_vec / len;
    // Two unit vectors perpendicular to the axis.
    let mut helper = Vector3::new(0.0, 1.0, 0.0);
    if axis.dot(helper).abs() > 0.99 {
        helper = Vector3::new(1.0, 0.0, 0.0);
    }
    let u = axis.cross(helper).normalize();
    let v = axis.cross(u).normalize();

    let ring_a = ring_points(a, u, v, radius);
    let ring_b = ring_points(b, u, v, radius);

    // End rings.
    push_ring(verts, xform, &ring_a);
    push_ring(verts, xform, &ring_b);
    // Longitudinal lines at 4 evenly spaced angles.
    for k in (0..WIRE_SEGMENTS).step_by(WIRE_SEGMENTS / 4) {
        push_segment(verts, xform, ring_a[k], ring_b[k]);
    }
    // Hemispherical caps: semicircle arcs over each end, in the (axis,u) and
    // (axis,v) planes, bulging away from the segment.
    let arc = |center: Vector3<f32>, plane: Vector3<f32>, cap_dir: Vector3<f32>| {
        let mut pts = Vec::with_capacity(WIRE_SEGMENTS + 1);
        for k in 0..=WIRE_SEGMENTS {
            let t = (k as f32) / (WIRE_SEGMENTS as f32) * std::f32::consts::PI;
            pts.push(center + (plane * t.cos() + cap_dir * t.sin()) * radius);
        }
        pts
    };
    for (center, cap_dir) in [(a, -axis), (b, axis)] {
        for plane in [u, v] {
            let pts = arc(center, plane, cap_dir);
            for k in 0..WIRE_SEGMENTS {
                push_segment(verts, xform, pts[k], pts[k + 1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three great circles, each a closed loop of `WIRE_SEGMENTS` segments -
    /// two vertices per segment, every vertex on the sphere - so the zone
    /// reads as a sphere from any angle rather than as a single ring.
    #[test]
    fn a_wire_sphere_is_three_closed_rings_on_the_sphere() {
        let center = Vector3::new(1.0, 2.0, 3.0);
        let verts = wire_sphere_segments(center, 0.25);
        assert_eq!(verts.len(), 3 * WIRE_SEGMENTS * 2);
        for vertex in &verts {
            let r = (vertex.position - center).magnitude();
            assert!((r - 0.25).abs() < 1e-5, "vertex off the sphere: r = {r}");
        }
    }
}
