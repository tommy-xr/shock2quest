//! Shared unit cone: base of radius one at Z=0 (capped), apex at Z=1.
use super::{Geometry, Mesh, VertexPositionTextureNormal, mesh};
use cgmath::{InnerSpace, vec2, vec3};
use once_cell::sync::OnceCell;

pub struct Cone;
static CONE: OnceCell<Mesh> = OnceCell::new();

const SIDES: usize = 16;

fn vertices() -> Vec<VertexPositionTextureNormal> {
    let mut out = Vec::with_capacity(SIDES * 6);
    // A side normal for radius = height leans 45 degrees up from the base.
    let side_normal = |angle: f32| vec3(angle.cos(), angle.sin(), 1.0).normalize();
    let rim = |i: usize| {
        let u = i as f32 / SIDES as f32;
        let angle = u * std::f32::consts::TAU;
        (vec3(angle.cos(), angle.sin(), 0.0), angle, u)
    };
    for i in 0..SIDES {
        let (a, angle_a, u_a) = rim(i);
        let (b, angle_b, u_b) = rim(i + 1);
        let apex = VertexPositionTextureNormal {
            position: vec3(0.0, 0.0, 1.0),
            normal: side_normal((angle_a + angle_b) * 0.5),
            uv: vec2((u_a + u_b) * 0.5, 1.0),
        };
        out.extend([
            VertexPositionTextureNormal {
                position: a,
                normal: side_normal(angle_a),
                uv: vec2(u_a, 0.0),
            },
            VertexPositionTextureNormal {
                position: b,
                normal: side_normal(angle_b),
                uv: vec2(u_b, 0.0),
            },
            apex,
        ]);
        let down = vec3(0.0, 0.0, -1.0);
        let cap = |position| VertexPositionTextureNormal {
            position,
            normal: down,
            uv: vec2(0.5, 0.0),
        };
        out.extend([cap(vec3(0.0, 0.0, 0.0)), cap(b), cap(a)]);
    }
    out
}

impl Geometry for Cone {
    fn draw(&self) {
        CONE.get_or_init(|| mesh::create(vertices())).draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_cone_has_outward_winding_and_unit_bounds() {
        let v = vertices();
        assert_eq!(v.len(), SIDES * 6);
        for triangle in v.chunks_exact(3) {
            let face = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            let centroid =
                (triangle[0].position + triangle[1].position + triangle[2].position) / 3.0;
            // Outward: away from a point inside the cone.
            assert!(face.dot(centroid - vec3(0.0, 0.0, 0.25)) > 0.0);
            for vertex in triangle {
                assert!(vertex.normal.dot(face) > 0.0);
                assert!((0.0..=1.0).contains(&vertex.position.z));
                assert!(vertex.position.truncate().magnitude() <= 1.00001);
            }
        }
        // One apex per side at +Z=1, and a base ring of radius one at Z=0.
        let apexes = v
            .iter()
            .filter(|vertex| vertex.position == vec3(0.0, 0.0, 1.0));
        assert_eq!(apexes.count(), SIDES);
        assert!(
            v.iter()
                .filter(|vertex| vertex.position.z == 0.0 && vertex.position != vec3(0.0, 0.0, 0.0))
                .all(|vertex| (vertex.position.truncate().magnitude() - 1.0).abs() < 1e-5)
        );
    }
}
