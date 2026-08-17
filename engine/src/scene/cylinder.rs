extern crate gl;
use cgmath::{Vector3, vec2, vec3};
use once_cell::sync::OnceCell;

pub use crate::scene::Geometry;

use super::{Mesh, VertexPositionTextureNormal, mesh};

/// A capped unit cylinder: radius 0.5 around the Z axis, spanning z = 0..1.
///
/// The z = 0 end (rather than the centre) is the origin so a caller can place a
/// tube by scaling `(diameter, diameter, length)` and translating that end to
/// where the tube starts - which is how the VR forearm hangs off the wrist.
///
/// `u` wraps once around the circumference and `v` runs along the axis.
pub struct Cylinder;

/// Sides around the tube. Sixteen is smooth enough for an arm-sized tube at
/// arm's length and stays cheap on the Quest (16 * 4 = 64 triangles).
const SEGMENTS: usize = 16;

static CYLINDER_GEOMETRY: OnceCell<Mesh> = OnceCell::new();

pub fn create() -> Cylinder {
    Cylinder
}

impl Geometry for Cylinder {
    fn draw(&self) {
        let mesh = CYLINDER_GEOMETRY.get_or_init(|| {
            let vertex = |normal: Vector3<f32>, z: f32, u: f32| VertexPositionTextureNormal {
                position: vec3(normal.x * 0.5, normal.y * 0.5, z),
                uv: vec2(u, z),
                normal,
            };

            let mut vertices: Vec<VertexPositionTextureNormal> = Vec::new();
            for segment in 0..SEGMENTS {
                let u0 = segment as f32 / SEGMENTS as f32;
                let u1 = (segment + 1) as f32 / SEGMENTS as f32;
                let angle = |u: f32| u * std::f32::consts::TAU;
                let radial = |u: f32| vec3(angle(u).cos(), angle(u).sin(), 0.0);
                let (n0, n1) = (radial(u0), radial(u1));

                // Side quad.
                vertices.extend([
                    vertex(n0, 0.0, u0),
                    vertex(n1, 0.0, u1),
                    vertex(n1, 1.0, u1),
                    vertex(n0, 0.0, u0),
                    vertex(n1, 1.0, u1),
                    vertex(n0, 1.0, u0),
                ]);

                // End caps, as fans from the axis. Both ends are capped so the
                // tube reads as solid from any angle, including from inside the
                // hand it is attached to.
                for (z, cap_normal) in [(0.0, vec3(0.0, 0.0, -1.0)), (1.0, vec3(0.0, 0.0, 1.0))] {
                    let rim = |radial: Vector3<f32>, u: f32| VertexPositionTextureNormal {
                        normal: cap_normal,
                        ..vertex(radial, z, u)
                    };
                    vertices.extend([
                        VertexPositionTextureNormal {
                            position: vec3(0.0, 0.0, z),
                            uv: vec2(0.5, z),
                            normal: cap_normal,
                        },
                        rim(n0, u0),
                        rim(n1, u1),
                    ]);
                }
            }

            mesh::create(vertices)
        });

        mesh.draw();
    }
}
