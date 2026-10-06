//! A ladder's hand holds, read off its render mesh: the rungs (horizontal
//! bars, or a pole ladder's side pegs) and the rails (vertical members, or the
//! pole). Its physics body is a single box, so the mesh is the only place the
//! rungs exist. A VR hand holds a ladder only on these (see
//! [`crate::physics::PhysicsWorld::set_ladder_holds`]); debug tooling reads
//! them too, to put a test hand on a real rung.

use cgmath::{InnerSpace, Matrix4, Transform, Vector3, point3, vec3};
use dark::properties::PropModelName;
use engine::assets::{
    asset_cache::AssetCache, asset_importer::AssetImporter, asset_paths::ReadableAndSeekable,
};
use once_cell::sync::Lazy;
use shipyard::{EntityId, Get, View, World};

use crate::runtime_props::RuntimePropTransform;

/// A straight member of a ladder, as its two end points.
pub type Segment = [Vector3<f32>; 2];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LadderHolds {
    /// Rungs bottom to top, each from its low-x end to its high-x end.
    pub rungs: Vec<Segment>,
    /// Rails by x, each from bottom to top.
    pub rails: Vec<Segment>,
}

/// The model member selected by a snapped grip, kept distinct from the collider face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Rung,
    Rail,
}

#[derive(Debug, Clone, Copy)]
pub struct LadderMember {
    pub kind: MemberKind,
    pub axis: Vector3<f32>,
    pub face_normal: Vector3<f32>,
}

/// Faces whose extents along a member's key axis (y for rungs, x for rails)
/// overlap or come this close fold into one member: a rung's cylinder faces,
/// a rail's segments.
const MERGE_DISTANCE: f32 = 0.1;

/// A member thicker than this across (about 19 cm) is no bar a hand can
/// close around - an organic strand, a panel - so its mesh is not a bar
/// ladder and offers no holds.
const MAX_BAR_THICKNESS: f32 = 0.25;

type Bounds = (Vector3<f32>, Vector3<f32>);

fn grow((lo, hi): Bounds, v: Vector3<f32>) -> Bounds {
    (
        vec3(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z)),
        vec3(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z)),
    )
}

const EMPTY: Bounds = (
    vec3(f32::MAX, f32::MAX, f32::MAX),
    vec3(f32::MIN, f32::MIN, f32::MIN),
);

/// Sweep faces in key order into members, so the result does not depend on
/// the mesh's polygon order.
fn merge(mut faces: Vec<Bounds>, key: fn(Vector3<f32>) -> f32) -> Vec<Bounds> {
    faces.sort_by(|a, b| key(a.0).total_cmp(&key(b.0)));
    let mut members: Vec<Bounds> = vec![];
    for (lo, hi) in faces {
        match members.last_mut() {
            Some(member) if key(lo) <= key(member.1) + MERGE_DISTANCE => {
                *member = grow(grow(*member, lo), hi);
            }
            _ => members.push((lo, hi)),
        }
    }
    members
}

/// Classify each polygon by its bounds: a rung face is long in x (at least a
/// quarter of the ladder's width) and flat in y; a rail face is long in y and
/// narrow in x (a slab's side faces count, so a slab offers its edges). Faces
/// that are neither (caps, feet) are ignored, and so is a "rung" with no
/// thickness (a slab's top face). A mesh with a member too thick to grip
/// offers none.
pub fn ladder_holds(vertices: &[Vector3<f32>], polygons: &[Vec<u16>]) -> LadderHolds {
    let bounds = |indices: &[u16]| {
        indices
            .iter()
            .filter_map(|&i| vertices.get(i as usize))
            .fold(EMPTY, |b, &v| grow(b, v))
    };
    let (lo, hi) = vertices.iter().fold(EMPTY, |b, &v| grow(b, v));
    let width = hi.x - lo.x;

    let (mut rung_faces, mut rail_faces) = (vec![], vec![]);
    for polygon in polygons {
        let (lo, hi) = bounds(polygon);
        let (dx, dy) = (hi.x - lo.x, hi.y - lo.y);
        if dx >= 0.25 * width && dy < dx / 4.0 {
            rung_faces.push((lo, hi));
        } else if dy > 2.0 * dx && dy >= 0.2 && dx <= 0.25 * width {
            rail_faces.push((lo, hi));
        }
    }
    let rungs = merge(rung_faces, |v| v.y);
    let rails = merge(rail_faces, |v| v.x);

    let rungs: Vec<Bounds> = rungs
        .into_iter()
        .filter(|(a, b)| b.y - a.y > 0.01)
        .collect();
    let too_thick = |(a, b): &Bounds, across: fn(Vector3<f32>) -> f32| {
        across(b - a).max(b.z - a.z) > MAX_BAR_THICKNESS
    };
    if rungs.iter().any(|r| too_thick(r, |v| v.y)) || rails.iter().any(|r| too_thick(r, |v| v.x)) {
        return LadderHolds::default();
    }

    // A rung is its bar's centre line; a rail its member's centre line.
    let mut rungs: Vec<Segment> = rungs
        .into_iter()
        .map(|(a, b)| {
            let (y, z) = ((a.y + b.y) / 2.0, (a.z + b.z) / 2.0);
            [vec3(a.x, y, z), vec3(b.x, y, z)]
        })
        .collect();
    let mut rails: Vec<Segment> = rails
        .into_iter()
        .map(|(a, b)| {
            let (x, z) = ((a.x + b.x) / 2.0, (a.z + b.z) / 2.0);
            [vec3(x, a.y, z), vec3(x, b.y, z)]
        })
        .collect();
    rungs.sort_by(|a, b| a[0].y.total_cmp(&b[0].y));
    rails.sort_by(|a, b| a[0].x.total_cmp(&b[0].x));
    LadderHolds { rungs, rails }
}

impl LadderHolds {
    pub fn is_empty(&self) -> bool {
        self.rungs.is_empty() && self.rails.is_empty()
    }

    /// The point on the nearest rung or rail to `hand`, if one is within
    /// `reach` of it.
    pub fn nearest_hold(&self, hand: Vector3<f32>, reach: f32) -> Option<Vector3<f32>> {
        self.rungs
            .iter()
            .chain(&self.rails)
            .map(|&[a, b]| crate::pathfinding::closest_point_on_segment(a, b, hand))
            .map(|p| (p, (p - hand).magnitude()))
            .filter(|&(_, distance)| distance <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// Recover the held member from its snapped point, using the same ordering
    /// and distance policy as acquisition. Rungs win an exact rung/rail tie.
    pub fn member_at(&self, point: Vector3<f32>) -> Option<LadderMember> {
        let normal = self
            .rungs
            .first()
            .zip(self.rails.first())
            .and_then(|(rung, rail)| {
                let normal = (rung[1] - rung[0]).cross(rail[1] - rail[0]);
                (normal.magnitude2() > 1e-8).then(|| normal.normalize())
            })?;
        self.rungs
            .iter()
            .map(|s| (MemberKind::Rung, s))
            .chain(self.rails.iter().map(|s| (MemberKind::Rail, s)))
            .map(|(kind, &[a, b])| {
                (
                    kind,
                    b - a,
                    (crate::pathfinding::closest_point_on_segment(a, b, point) - point)
                        .magnitude2(),
                )
            })
            .filter(|(_, axis, distance)| axis.magnitude2() > 1e-8 && *distance < 1e-6)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(kind, axis, _)| LadderMember {
                kind,
                axis: axis.normalize(),
                face_normal: normal,
            })
    }

    /// These holds placed in the world by the ladder entity's transform.
    pub fn transformed(&self, transform: &Matrix4<f32>) -> LadderHolds {
        let place = |segments: &[Segment]| {
            segments
                .iter()
                .map(|s| {
                    s.map(|v| {
                        let p = transform.transform_point(point3(v.x, v.y, v.z));
                        vec3(p.x, p.y, p.z)
                    })
                })
                .collect()
        };
        LadderHolds {
            rungs: place(&self.rungs),
            rails: place(&self.rails),
        }
    }
}

/// The ladder face's normal in the world: the model's +z, which is its thin
/// axis. Either side may be the climbing side.
pub fn face_normal(transform: &Matrix4<f32>) -> Vector3<f32> {
    transform.transform_vector(Vector3::unit_z()).normalize()
}

/// A `.bin` model's holds in model space, cached per model by the asset
/// cache. Empty for a model that is not an object model.
static LADDER_HOLDS_IMPORTER: Lazy<AssetImporter<LadderHolds, LadderHolds, ()>> = Lazy::new(|| {
    AssetImporter::define(
        |_name, reader: &mut Box<dyn ReadableAndSeekable>, _assets, _config| {
            let header = dark::ss2_bin_header::read(reader);
            if !matches!(header.bin_type, dark::ss2_bin_header::BinFileType::Obj) {
                return LadderHolds::default();
            }
            // Raw vertices, no sub-object palette: every ladder model is one part.
            let mesh = dark::ss2_bin_obj_loader::read(reader, &header);
            let polygons: Vec<Vec<u16>> = mesh
                .polygons
                .iter()
                .map(|p| p.vertex_indices.clone())
                .collect();
            ladder_holds(&mesh.vertices, &polygons)
        },
        |holds, _assets, _config| holds,
    )
});

/// `entity`'s model name, its holds placed in the world, and its face normal;
/// `None` when it has no model, no transform, or its `.bin` cannot be opened.
pub fn entity_holds(
    asset_cache: &mut AssetCache,
    world: &World,
    entity: EntityId,
) -> Option<(String, LadderHolds, Vector3<f32>)> {
    let (model, transform) = world.run(
        |names: View<PropModelName>, transforms: View<RuntimePropTransform>| {
            Some((
                names.get(entity).ok()?.0.clone(),
                transforms.get(entity).ok()?.0,
            ))
        },
    )?;
    let holds = asset_cache.get_opt(&LADDER_HOLDS_IMPORTER, &format!("{model}.bin"))?;
    Some((
        model,
        holds.transformed(&transform),
        face_normal(&transform),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed box from `lo` to `hi`, as its six faces.
    fn bar(vertices: &mut Vec<Vector3<f32>>, lo: Vector3<f32>, hi: Vector3<f32>) -> Vec<Vec<u16>> {
        let base = vertices.len() as u16;
        for &x in &[lo.x, hi.x] {
            for &y in &[lo.y, hi.y] {
                for &z in &[lo.z, hi.z] {
                    vertices.push(vec3(x, y, z));
                }
            }
        }
        // Corner index = base + 4*xi + 2*yi + zi.
        let face = |a: u16, b: u16, c: u16, d: u16| vec![base + a, base + b, base + c, base + d];
        vec![
            face(0, 1, 3, 2), // x-lo
            face(4, 5, 7, 6), // x-hi
            face(0, 1, 5, 4), // y-lo
            face(2, 3, 7, 6), // y-hi
            face(0, 2, 6, 4), // z-lo
            face(1, 3, 7, 5), // z-hi
        ]
    }

    #[test]
    fn a_rail_ladder_yields_its_rungs_and_both_rails() {
        let mut vertices = vec![];
        let mut polygons = vec![];
        for x in [-0.44, 0.36] {
            polygons.extend(bar(
                &mut vertices,
                vec3(x, -1.6, -0.04),
                vec3(x + 0.08, 1.6, 0.04),
            ));
        }
        for y in [-0.8, 0.0, 0.8] {
            polygons.extend(bar(
                &mut vertices,
                vec3(-0.36, y - 0.04, -0.04),
                vec3(0.36, y + 0.04, 0.04),
            ));
        }

        let holds = ladder_holds(&vertices, &polygons);
        let heights: Vec<f32> = holds.rungs.iter().map(|r| r[0].y).collect();
        assert_eq!(heights, vec![-0.8, 0.0, 0.8]);
        assert_eq!(
            holds.rungs[0],
            [vec3(-0.36, -0.8, 0.0), vec3(0.36, -0.8, 0.0)]
        );
        let rails: Vec<f32> = holds.rails.iter().map(|r| r[0].x).collect();
        assert_eq!(rails, vec![-0.4, 0.4]);
        assert_eq!(
            holds.rails[0],
            [vec3(-0.4, -1.6, 0.0), vec3(-0.4, 1.6, 0.0)]
        );
    }

    #[test]
    fn a_pole_ladder_yields_its_pegs_and_the_pole() {
        let mut vertices = vec![];
        let mut polygons = bar(
            &mut vertices,
            vec3(-0.08, -1.6, -0.08),
            vec3(0.08, 1.6, 0.08),
        );
        polygons.extend(bar(
            &mut vertices,
            vec3(0.03, -1.0, -0.04),
            vec3(0.82, -0.86, 0.04),
        ));
        polygons.extend(bar(
            &mut vertices,
            vec3(-0.82, 0.0, -0.04),
            vec3(-0.03, 0.14, 0.04),
        ));

        let holds = ladder_holds(&vertices, &polygons);
        assert_eq!(holds.rungs.len(), 2);
        assert_eq!(holds.rungs[0][0].x, 0.03, "the low peg sticks out to +x");
        assert_eq!(holds.rungs[1][1].x, -0.03, "the high peg sticks out to -x");
        assert_eq!(holds.rails.len(), 1, "the pole's faces are one rail");
        assert_eq!(holds.rails[0][0].x, 0.0);

        // Polygon order does not matter.
        for order in [polygons.iter().rev().cloned().collect::<Vec<_>>(), {
            let mut odd_first: Vec<_> = polygons.iter().skip(1).step_by(2).cloned().collect();
            odd_first.extend(polygons.iter().step_by(2).cloned());
            odd_first
        }] {
            assert_eq!(ladder_holds(&vertices, &order), holds);
        }
    }

    #[test]
    fn a_slab_has_rails_but_no_rungs() {
        let mut vertices = vec![];
        let polygons = bar(
            &mut vertices,
            vec3(-0.45, -1.6, -0.05),
            vec3(0.45, 1.6, 0.05),
        );
        // Its broad faces are neither bars nor uprights; its top is flat.
        let holds = ladder_holds(&vertices, &polygons);
        assert!(holds.rungs.is_empty());
        let edges: Vec<f32> = holds.rails.iter().map(|r| r[0].x).collect();
        assert_eq!(edges, vec![-0.45, 0.45]);
    }

    #[test]
    fn a_mesh_with_members_too_thick_to_grip_offers_no_holds() {
        // Many's nerve climbable: strands about 0.65 across.
        let mut vertices = vec![];
        let mut polygons = vec![];
        for x in [-1.6, -0.33, 0.96] {
            polygons.extend(bar(
                &mut vertices,
                vec3(x, -1.6, -0.03),
                vec3(x + 0.64, 1.6, 0.2),
            ));
        }
        assert!(ladder_holds(&vertices, &polygons).is_empty());
    }

    #[test]
    fn transformed_holds_follow_the_entity() {
        let holds = LadderHolds {
            rungs: vec![[vec3(-0.4, 0.0, 0.0), vec3(0.4, 0.0, 0.0)]],
            rails: vec![],
        };
        let transform = Matrix4::from_translation(vec3(10.0, 2.0, 0.0))
            * Matrix4::from_angle_y(cgmath::Deg(90.0));
        let placed = holds.transformed(&transform);
        assert!((placed.rungs[0][0] - vec3(10.0, 2.0, 0.4)).magnitude() < 1e-5);
        assert!((face_normal(&transform) - vec3(1.0, 0.0, 0.0)).magnitude() < 1e-5);
    }
    #[test]
    fn snapped_points_retain_their_model_member_and_rotated_axis() {
        let holds = LadderHolds {
            rungs: vec![[vec3(-1.0, 1.0, 0.0), vec3(1.0, 1.0, 0.0)]],
            rails: vec![[vec3(1.0, 0.0, 0.0), vec3(1.0, 2.0, 0.0)]],
        };
        assert_eq!(
            holds.member_at(vec3(0.0, 1.0, 0.0)).unwrap().kind,
            MemberKind::Rung
        );
        assert_eq!(
            holds.member_at(vec3(1.0, 1.5, 0.0)).unwrap().kind,
            MemberKind::Rail
        );
        assert!(holds.member_at(vec3(0.0, 1.5, 0.0)).is_none());
        let rotated = holds.transformed(&Matrix4::from_angle_y(cgmath::Deg(90.0)));
        let rung = rotated.member_at(vec3(0.0, 1.0, 0.0)).unwrap();
        assert!(rung.axis.dot(-Vector3::unit_z()) > 0.999);
        assert!(rung.face_normal.dot(Vector3::unit_x()) > 0.999);
    }
}
