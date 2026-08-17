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

use cgmath::{Matrix4, Quaternion, Vector3, vec2, vec3};
use dark::importers::TEXTURE_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{Geometry, Mesh, SceneObject, VertexPositionTextureNormal, mesh},
};

/// The game's first-person hand texture; its top band is the suit cuff.
const SLEEVE_TEXTURE: &str = "FISTCOMP.PCX";

/// The cuff band inside that texture, in `v`. Measured from the asset: the
/// ribbed sleeve occupies the top ~42%, bare skin the rest. The tube samples
/// the band back-to-front - `SLEEVE_V_AT_WRIST` at the wrist, so the cuff's
/// edge meets the hand, running deeper into the sleeve toward the elbow.
const SLEEVE_V_AT_WRIST: f32 = 0.40;
const SLEEVE_V_AT_ELBOW: f32 = 0.02;

/// Where the tube starts and ends along the hand's local +Z (the elbow
/// direction - the hand's forward is -Z), in meters from the wrist.
///
/// It starts slightly *inside* the hand so a turning wrist can never open a gap
/// at the cuff, and it has to end before the forearm HUD panel's **near edge**
/// (the panel lies along the arm, centred on its axis) or the tube swallows the
/// panel. `forearm_clears_the_forearm_hud_panel` guards that against both
/// numbers moving.
const FOREARM_START_METERS: f32 = -0.02;
const FOREARM_END_METERS: f32 = 0.085;

/// Forearm thickness - a little over a wrist, a little under a forearm.
const FOREARM_RADIUS_METERS: f32 = 0.032;

/// Sides around the tube. Sixteen is smooth enough for an arm-sized tube at
/// arm's length and stays cheap on the Quest (16 * 4 = 64 triangles).
const SEGMENTS: usize = 16;

thread_local! {
    static FOREARM_MESH: once_cell::unsync::OnceCell<Mesh> =
        const { once_cell::unsync::OnceCell::new() };
}

/// A capped tube of unit length and unit diameter around +Z, spanning
/// z = 0..1, with the sleeve band baked into its UVs.
struct ForearmTube;

impl Geometry for ForearmTube {
    fn draw(&self) {
        FOREARM_MESH.with(|cell| cell.get_or_init(|| mesh::create(build_vertices())).draw());
    }
}

/// The tube as plain data - separate from `draw` so its winding and UVs are
/// testable without a GL context.
fn build_vertices() -> Vec<VertexPositionTextureNormal> {
    let angle = |u: f32| u * std::f32::consts::TAU;
    let radial = |u: f32| vec3(angle(u).cos(), angle(u).sin(), 0.0);
    // `u` wraps once around the tube, staying off the texture's edge columns;
    // `v` runs the sleeve band from its cuff edge (wrist) inward (elbow).
    let sleeve_uv = |u: f32, z: f32| {
        vec2(
            0.1 + 0.8 * u,
            SLEEVE_V_AT_WRIST + z * (SLEEVE_V_AT_ELBOW - SLEEVE_V_AT_WRIST),
        )
    };
    let side = |normal: Vector3<f32>, z: f32, u: f32| VertexPositionTextureNormal {
        position: vec3(normal.x * 0.5, normal.y * 0.5, z),
        uv: sleeve_uv(u, z),
        normal,
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
        for (z, cap_normal) in [(0.0, vec3(0.0, 0.0, -1.0)), (1.0, vec3(0.0, 0.0, 1.0))] {
            let cap = |position: Vector3<f32>| VertexPositionTextureNormal {
                position,
                uv: sleeve_uv(0.5, z),
                normal: cap_normal,
            };
            let centre = cap(vec3(0.0, 0.0, z));
            let (rim0, rim1) = (cap(side(n0, z, u0).position), cap(side(n1, z, u1).position));
            // Wound so each face's geometric normal agrees with `cap_normal`;
            // the z = 0 fan has to run the other way round for that, and only
            // backface culling would ever tell you it didn't.
            vertices.extend(if z == 0.0 {
                [centre, rim1, rim0]
            } else {
                [centre, rim0, rim1]
            });
        }
    }

    vertices
}

/// The sleeve texture, or `None` on an install that has neither an upgraded nor
/// the original encoding of it - in which case the hand renders without a
/// forearm rather than with an untextured one.
pub fn load_sleeve(asset_cache: &mut AssetCache) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    let resolved = dark::util::resolve_texture_name(asset_cache, SLEEVE_TEXTURE)?;
    asset_cache
        .get_opt::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, &resolved)
        .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>)
}

/// Build the forearm once, at the identity transform. Callers clone it per hand
/// per frame (sharing its material and geometry) and set the transform, rather
/// than rebuilding a material every frame for every hand.
pub fn template(texture: Rc<dyn engine::texture::TextureTrait>) -> SceneObject {
    SceneObject::create(
        RefCell::new(engine::scene::basic_material::create(texture, 1.0, 0.0)),
        Rc::new(Box::new(ForearmTube) as Box<dyn Geometry>),
    )
}

/// Place a cloned [`template`] as the forearm of a hand at
/// `position`/`rotation`: the unit tube scaled to the arm's thickness and
/// length, then pushed along the hand's local +Z so it starts inside the wrist.
pub fn transform(position: Vector3<f32>, rotation: Quaternion<f32>) -> Matrix4<f32> {
    let to_world_units = |meters: f32| meters / crate::METERS_PER_WORLD_UNIT;
    let start = to_world_units(FOREARM_START_METERS);
    let length = to_world_units(FOREARM_END_METERS - FOREARM_START_METERS);
    let diameter = to_world_units(2.0 * FOREARM_RADIUS_METERS);

    Matrix4::from_translation(position)
        * Matrix4::from(rotation)
        * Matrix4::from_translation(Vector3::new(0.0, 0.0, start))
        * Matrix4::from_nonuniform_scale(diameter, diameter, length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{EuclideanSpace, InnerSpace, Point3, Transform};

    /// The forearm has to start *inside* the hand, or a rotating wrist opens a
    /// visible gap between the tube and the hand it belongs to.
    #[test]
    fn forearm_starts_behind_the_wrist_and_runs_toward_the_elbow() {
        let position = Vector3::new(1.0, 2.0, 3.0);
        let placement = transform(position, Quaternion::new(1.0, 0.0, 0.0, 0.0));
        let end_of = |z: f32| placement.transform_point(Point3::new(0.0, 0.0, z)).to_vec();

        // The hand's forward is -Z, so the elbow end must be at greater z than
        // the wrist end, which itself sits behind the hand origin.
        assert!(end_of(0.0).z < position.z, "forearm starts past the wrist");
        assert!(end_of(1.0).z > end_of(0.0).z, "forearm runs the wrong way");

        let length_meters = (end_of(1.0) - end_of(0.0)).z * crate::METERS_PER_WORLD_UNIT;
        assert!(
            (length_meters - (FOREARM_END_METERS - FOREARM_START_METERS)).abs() < 1e-4,
            "forearm renders {length_meters} m long"
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
