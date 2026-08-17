//! The VR forearm: a short tube off the wrist wearing the player's own suit
//! sleeve, so the hands don't read as severed.
//!
//! The sleeve is not invented art - it is the cuff band of the game's own
//! first-person hand texture (`FISTCOMP.PCX`, the map the `*_h` hand models
//! wear: a ribbed suit cuff across the top of the atlas, bare skin below).
//! Resolving it through [`dark::util::resolve_texture_name`] means a 25th
//! Anniversary or mod install's upgraded encoding of that same texture wins
//! automatically, and a classic install still finds the original in `obj.crf`.
//!
//! There is no elbow and no IK: the tube follows the wrist rigidly, exactly
//! like the forearm HUD panels do.

use std::cell::RefCell;
use std::rc::Rc;

use cgmath::{InnerSpace, Matrix4, Quaternion, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{Geometry, SceneObject, VertexPositionTextureNormal, mesh},
};

/// The game's first-person hand texture; its top band is the suit cuff.
const SLEEVE_TEXTURE: &str = "FISTCOMP.PCX";

/// The cuff band inside that texture, in `v`. Measured from the asset: the
/// ribbed sleeve occupies the top ~42%, bare skin the rest. The tube samples
/// the band back-to-front - `SLEEVE_V_AT_WRIST` at the wrist, so the cuff's
/// edge meets the hand, running deeper into the sleeve toward the elbow.
const SLEEVE_V_AT_WRIST: f32 = 0.40;
const SLEEVE_V_AT_ELBOW: f32 = 0.02;

/// Where the hand mesh itself ends, along the hand's local +Z (the elbow
/// direction - the hand's forward is -Z), in meters behind the hand origin.
/// The tube's wrist end has to sit *just* inside that, not at the origin: the
/// origin is the wrist joint, and the mesh continues ~2.6 cm past it as a wrist
/// stub, so a tube starting at (or in front of) the origin runs several
/// centimetres up the inside of the hand and its cuff disc surfaces through the
/// back of the palm.
const HAND_MESH_END_METERS: f32 = crate::hand_glove::AUTHORED_WRIST_STUB_WORLD
    * crate::hand_glove::GLOVE_SCALE
    * crate::METERS_PER_WORLD_UNIT;

/// How far the cuff laps over the hand's wrist stub. Enough to hide the mesh's
/// open wrist hole and to survive the stub's rounding; small enough that the
/// cuff never reaches the palm.
const CUFF_OVERLAP_METERS: f32 = 0.01;

/// Where the tube starts and ends along the hand's local +Z, in meters from the
/// hand origin.
///
/// It has to end before the forearm HUD panel's **near edge** (the panel lies
/// along the arm, centred on its axis) or the tube swallows the panel.
/// `forearm_clears_the_forearm_hud_panel` guards that against both numbers
/// moving.
const FOREARM_START_METERS: f32 = HAND_MESH_END_METERS - CUFF_OVERLAP_METERS;
const FOREARM_END_METERS: f32 = 0.085;

/// Forearm thickness. The cuff end matches the hand mesh's own wrist so the two
/// meet without a step, and the tube widens gently toward the elbow - a real
/// forearm flares a little over this span, and much more reads as a megaphone.
const FOREARM_WRIST_RADIUS_METERS: f32 = 0.032;
const FOREARM_ELBOW_RADIUS_METERS: f32 = 0.035;

/// Sides around the tube. Sixteen is smooth enough for an arm-sized tube at
/// arm's length and stays cheap on the Quest (16 * 4 = 64 triangles).
const SEGMENTS: usize = 16;

/// Meters, as the renderer counts distance.
fn world_units(meters: f32) -> f32 {
    meters / crate::METERS_PER_WORLD_UNIT
}

/// The tube as plain data, in hand-local world units - separate from `draw` so
/// its shape, winding and UVs are testable without a GL context. Building it at
/// its real size (rather than as a unit tube the placement scales) is what lets
/// those tests measure what actually renders.
fn build_vertices() -> Vec<VertexPositionTextureNormal> {
    let angle = |u: f32| u * std::f32::consts::TAU;
    let radial = |u: f32| vec3(angle(u).cos(), angle(u).sin(), 0.0);
    // `u` wraps once around the tube, staying off the texture's edge columns;
    // `v` runs the sleeve band from its cuff edge (wrist) inward (elbow).
    let sleeve_uv = |u: f32, t: f32| {
        vec2(
            0.1 + 0.8 * u,
            SLEEVE_V_AT_WRIST + t * (SLEEVE_V_AT_ELBOW - SLEEVE_V_AT_WRIST),
        )
    };
    // `t` runs 0 (cuff) to 1 (elbow) along the tube.
    let length = FOREARM_END_METERS - FOREARM_START_METERS;
    let flare = FOREARM_ELBOW_RADIUS_METERS - FOREARM_WRIST_RADIUS_METERS;
    let z = |t: f32| world_units(FOREARM_START_METERS + t * length);
    let radius = |t: f32| world_units(FOREARM_WRIST_RADIUS_METERS + t * flare);
    // A cone's normal leans back along its slope; a radial one would shade the
    // taper as if it were still a cylinder, and the sleeve *is* lit
    // (`basic_material` runs a per-light `dot(normal, lightDir)`).
    let slope = flare / length;
    let side = |radial: Vector3<f32>, t: f32, u: f32| VertexPositionTextureNormal {
        position: vec3(radial.x * radius(t), radial.y * radius(t), z(t)),
        uv: sleeve_uv(u, t),
        normal: vec3(radial.x, radial.y, -slope).normalize(),
    };

    let mut vertices: Vec<VertexPositionTextureNormal> = Vec::new();
    for segment in 0..SEGMENTS {
        let u0 = segment as f32 / SEGMENTS as f32;
        let u1 = (segment + 1) as f32 / SEGMENTS as f32;
        let (n0, n1) = (radial(u0), radial(u1));

        vertices.extend([
            side(n0, 0.0, u0),
            side(n1, 0.0, u1),
            side(n1, 1.0, u1),
            side(n0, 0.0, u0),
            side(n1, 1.0, u1),
            side(n0, 1.0, u0),
        ]);

        // End caps, as fans from the axis. Both ends are capped so the tube
        // reads as solid from any angle, including from inside the hand.
        //
        // A cap samples ONE texel: `u` runs around the circumference, so
        // interpolating it across a fan wedge - degenerate at the hub - would
        // draw the sleeve's ribbing as concentric rings. One flat sleeve-dark
        // disc is what a tube cut off mid-sleeve should look like.
        for (t, cap_normal) in [(0.0, vec3(0.0, 0.0, -1.0)), (1.0, vec3(0.0, 0.0, 1.0))] {
            let cap = |position: Vector3<f32>| VertexPositionTextureNormal {
                position,
                uv: sleeve_uv(0.5, t),
                normal: cap_normal,
            };
            let centre = cap(vec3(0.0, 0.0, z(t)));
            let (rim0, rim1) = (cap(side(n0, t, u0).position), cap(side(n1, t, u1).position));
            // Wound so each face's geometric normal agrees with `cap_normal`;
            // the cuff-end fan has to run the other way round for that, and only
            // backface culling would ever tell you it didn't.
            vertices.extend(if t == 0.0 {
                [centre, rim1, rim0]
            } else {
                [centre, rim0, rim1]
            });
        }
    }

    vertices
}

/// Build the forearm once, at the identity transform, with its mesh and
/// material owned by the returned object: callers clone it per hand per frame
/// (the clone shares both) and only set the transform.
///
/// `None` on an install that has neither an upgraded nor the original encoding
/// of the sleeve texture - the hand then renders without a forearm rather than
/// with an untextured one.
pub fn template(asset_cache: &mut AssetCache) -> Option<SceneObject> {
    let texture = dark::util::load_texture_with_fallback(asset_cache, SLEEVE_TEXTURE)?
        as Rc<dyn engine::texture::TextureTrait>;

    Some(SceneObject::create(
        RefCell::new(engine::scene::basic_material::create(texture, 1.0, 0.0)),
        Rc::new(Box::new(mesh::create(build_vertices())) as Box<dyn Geometry>),
    ))
}

/// Place a cloned [`template`] as the forearm of a hand at
/// `position`/`rotation`. The mesh is already the arm's real size and sits
/// where it belongs along the hand's local +Z, so this only moves it onto the
/// hand.
pub fn transform(position: Vector3<f32>, rotation: Quaternion<f32>) -> Matrix4<f32> {
    Matrix4::from_translation(position) * Matrix4::from(rotation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{EuclideanSpace, InnerSpace, Point3, Transform};

    /// What the mesh actually renders as, in meters on the hand's own axis:
    /// (z at the cuff end, z at the elbow end, radius at the cuff end, radius
    /// at the elbow end). Measured through `transform`, so a placement that
    /// scaled or shifted the tube would show up here rather than in nothing.
    fn rendered_meters() -> (f32, f32, f32, f32) {
        let position = Vector3::new(1.0, 2.0, 3.0);
        let placement = transform(position, Quaternion::new(1.0, 0.0, 0.0, 0.0));
        let mut ends: Vec<(f32, f32)> = build_vertices()
            .iter()
            .filter(|vertex| vertex.normal.z.abs() < 0.5)
            .map(|vertex| {
                let world = placement
                    .transform_point(Point3::from_vec(vertex.position))
                    .to_vec()
                    - position;
                (
                    world.z * crate::METERS_PER_WORLD_UNIT,
                    world.truncate().magnitude() * crate::METERS_PER_WORLD_UNIT,
                )
            })
            .collect();
        ends.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (cuff, elbow) = (ends[0], ends[ends.len() - 1]);
        (cuff.0, elbow.0, cuff.1, elbow.1)
    }

    /// The cuff has to lap over the hand mesh's wrist stub - far enough that no
    /// gap opens where the mesh's wrist hole is, but not so far that it
    /// surfaces through the hand. Overshooting is invisible from straight ahead
    /// and obvious from the side, which is how it shipped in the first place.
    #[test]
    fn forearm_laps_the_wrist_stub_without_reaching_into_the_hand() {
        let (cuff_z, elbow_z, _, _) = rendered_meters();

        // The hand's forward is -Z, so the elbow end must be at greater z than
        // the cuff end.
        assert!(elbow_z > cuff_z, "forearm runs the wrong way");

        let overlap = HAND_MESH_END_METERS - cuff_z;
        assert!(
            (0.005..=0.015).contains(&overlap),
            "cuff laps the hand mesh by {overlap} m; it has to reach inside it (no gap) without reaching the palm"
        );

        let length = elbow_z - cuff_z;
        assert!(
            (length - (FOREARM_END_METERS - FOREARM_START_METERS)).abs() < 1e-4,
            "forearm renders {length} m long"
        );
    }

    /// The forearm HUD panel lies *along* the arm, centred on its axis, so a
    /// tube reaching past the panel's near edge swallows it. Guarding against
    /// the panel's centre (its offset) would pass while the two intersect.
    #[test]
    fn forearm_clears_the_forearm_hud_panel() {
        use crate::hud::virtual_arms::{FOREARM_OFFSET, HUD_PANEL_WIDTH};

        let panel_near_edge_meters =
            (FOREARM_OFFSET.z - HUD_PANEL_WIDTH / 2.0) * crate::METERS_PER_WORLD_UNIT;

        assert!(
            FOREARM_END_METERS < panel_near_edge_meters,
            "forearm reaches {FOREARM_END_METERS} m, the HUD panel starts at {panel_near_edge_meters} m"
        );
    }

    /// The cuff end has to render at the hand's own wrist width - a tube that
    /// arrives narrower leaves a step where the arm is thinner than the wrist,
    /// and one that flares hard reads as a megaphone rather than a forearm.
    #[test]
    fn the_cuff_end_is_wrist_width_and_the_flare_is_gentle() {
        let (_, _, cuff_radius, elbow_radius) = rendered_meters();

        assert!(
            (cuff_radius - FOREARM_WRIST_RADIUS_METERS).abs() < 1e-4,
            "cuff end renders at {cuff_radius} m"
        );
        assert!(
            (elbow_radius - FOREARM_ELBOW_RADIUS_METERS).abs() < 1e-4,
            "elbow end renders at {elbow_radius} m"
        );
        assert!(
            elbow_radius / cuff_radius <= 1.15,
            "the tube flares {:.0}% over its length",
            100.0 * (elbow_radius / cuff_radius - 1.0)
        );
    }

    /// Every triangle's geometric normal must agree with the normals it
    /// declares - a cap wound the wrong way round is invisible today (culling
    /// is off) and vanishes the moment anything turns it on.
    #[test]
    fn every_triangle_is_wound_to_match_its_normals() {
        let vertices = build_vertices();
        assert_eq!(vertices.len() % 3, 0);

        for (index, triangle) in vertices.chunks(3).enumerate() {
            let geometric = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[1].position)
                .normalize();
            let declared = triangle
                .iter()
                .fold(vec3(0.0, 0.0, 0.0), |sum, vertex| sum + vertex.normal)
                .normalize();

            assert!(
                geometric.dot(declared) > 0.0,
                "triangle {index} is wound against its normals: {geometric:?} vs {declared:?}"
            );
        }
    }

    /// Every texel the tube samples must come from the texture's sleeve band -
    /// reaching past it drags the hand-skin half of the atlas onto the arm.
    #[test]
    fn the_tube_only_ever_samples_the_sleeve_band() {
        for vertex in build_vertices() {
            assert!(
                (SLEEVE_V_AT_ELBOW..=SLEEVE_V_AT_WRIST).contains(&vertex.uv.y),
                "uv {:?} leaves the sleeve band",
                vertex.uv
            );
            assert!(
                (0.0..=1.0).contains(&vertex.uv.x),
                "uv {:?} leaves the texture",
                vertex.uv
            );
        }
    }

    /// The caps must sample a single texel: a `u` that varies across a fan
    /// wedge renders the sleeve's ribbing as concentric rings.
    #[test]
    fn cap_triangles_sample_one_texel() {
        for triangle in build_vertices().chunks(3) {
            let is_cap = triangle.iter().all(|vertex| vertex.normal.z.abs() > 0.5);
            if is_cap {
                assert!(
                    triangle.iter().all(|vertex| vertex.uv == triangle[0].uv),
                    "cap triangle interpolates its uv: {:?}",
                    triangle.iter().map(|v| v.uv).collect::<Vec<_>>()
                );
            }
        }
    }
}
