//! Shared unit UV sphere, radius one, centered on the origin.
use super::{Geometry, Mesh, VertexPositionTextureNormal, mesh};
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

pub struct Sphere;
static SPHERE: OnceCell<Mesh> = OnceCell::new();

const SEGMENTS: usize = 32;
const RINGS: usize = 16;

fn vertices() -> Vec<VertexPositionTextureNormal> {
    // u around +Y, v from the south pole (0) to the north pole (1).
    let point = |i: usize, j: usize| {
        let u = i as f32 / SEGMENTS as f32;
        let v = j as f32 / RINGS as f32;
        let (theta, phi) = (u * std::f32::consts::TAU, v * std::f32::consts::PI);
        let normal = vec3(phi.sin() * theta.cos(), -phi.cos(), phi.sin() * theta.sin());
        VertexPositionTextureNormal {
            position: normal,
            normal,
            uv: vec2(u, v),
        }
    };
    let mut out = Vec::with_capacity(SEGMENTS * RINGS * 6);
    for j in 0..RINGS {
        for i in 0..SEGMENTS {
            let (a, b, c, d) = ((i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1));
            out.extend([a, c, b, a, d, c].map(|(i, j)| point(i, j)));
        }
    }
    out
}

impl Geometry for Sphere {
    fn draw(&self) {
        SPHERE.get_or_init(|| mesh::create(vertices())).draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    #[test]
    fn unit_sphere_has_outward_winding() {
        let v = vertices();
        assert_eq!(v.len(), SEGMENTS * RINGS * 6);
        let mut outward = 0;
        for triangle in v.chunks_exact(3) {
            for vertex in triangle {
                assert!((vertex.position.magnitude() - 1.0).abs() < 1e-5);
            }
            let normal = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            // Pole triangles are degenerate; every other face winds outward.
            if normal.magnitude() > 1e-6 {
                let center =
                    (triangle[0].position + triangle[1].position + triangle[2].position) / 3.0;
                assert!(normal.dot(center) > 0.0);
                outward += 1;
            }
        }
        assert_eq!(outward, SEGMENTS * RINGS * 2 - SEGMENTS * 2);
    }
}
