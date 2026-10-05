//! Shared unit cylinder: radius one, extending along +Z from zero to one.
use super::{Geometry, Mesh, VertexPositionTextureNormal, mesh};
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

pub struct Cylinder;
static CYLINDER: OnceCell<Mesh> = OnceCell::new();

fn vertices() -> Vec<VertexPositionTextureNormal> {
    const SIDES: usize = 16;
    let mut out = Vec::with_capacity(SIDES * 12);
    let ring = |i: usize, z: f32| {
        let u = i as f32 / SIDES as f32;
        let angle = u * std::f32::consts::TAU;
        VertexPositionTextureNormal {
            position: vec3(angle.cos(), angle.sin(), z),
            normal: vec3(angle.cos(), angle.sin(), 0.0),
            uv: vec2(u, z),
        }
    };
    for i in 0..SIDES {
        out.extend([
            ring(i, 0.0),
            ring(i + 1, 0.0),
            ring(i + 1, 1.0),
            ring(i, 0.0),
            ring(i + 1, 1.0),
            ring(i, 1.0),
        ]);
        for z in [0.0, 1.0] {
            let normal = vec3(0.0, 0.0, if z == 0.0 { -1.0 } else { 1.0 });
            let mut a = ring(i, z);
            let mut b = ring(i + 1, z);
            a.normal = normal;
            b.normal = normal;
            let center = VertexPositionTextureNormal {
                position: vec3(0.0, 0.0, z),
                normal,
                uv: vec2(0.5, z),
            };
            if z == 0.0 {
                out.extend([center, b, a]);
            } else {
                out.extend([center, a, b]);
            }
        }
    }
    out
}

impl Geometry for Cylinder {
    fn draw(&self) {
        CYLINDER.get_or_init(|| mesh::create(vertices())).draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;
    #[test]
    fn closed_cylinder_has_outward_winding_and_unit_bounds() {
        let v = vertices();
        assert_eq!(v.len(), 192);
        for triangle in v.chunks_exact(3) {
            let normal = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            assert!(normal.dot(triangle[1].normal) > 0.0);
            for vertex in triangle {
                assert!((0.0..=1.0).contains(&vertex.position.z));
                assert!(vertex.position.truncate().magnitude() <= 1.00001);
            }
        }
    }
}
