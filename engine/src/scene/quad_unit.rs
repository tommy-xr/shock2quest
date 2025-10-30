extern crate gl;
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

pub use crate::scene::Geometry;

use super::{mesh, Mesh, VertexPositionTextureNormal};

pub struct QuadUnit;

static QUAD_UNIT_GEOMETRY: OnceCell<Mesh> = OnceCell::new();

pub fn create() -> QuadUnit {
    QuadUnit
}

impl Geometry for QuadUnit {
    fn draw(&self) {
        let mesh = QUAD_UNIT_GEOMETRY.get_or_init(|| {
            // Normal pointing forward (positive Z direction)
            let normal = vec3(0.0, 0.0, 1.0);

            let vertices: [VertexPositionTextureNormal; 6] = [
                // Tri 1
                VertexPositionTextureNormal {
                    position: vec3(0.0, 0.0, 0.0), // bottom-left
                    uv: vec2(0.0, 0.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(0.0, 1.0, 0.0), // top-left
                    uv: vec2(0.0, 1.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(1.0, 1.0, 0.0), // top-right
                    uv: vec2(1.0, 1.0),
                    normal,
                },
                // Tri 2
                VertexPositionTextureNormal {
                    position: vec3(1.0, 0.0, 0.0), // bottom-right
                    uv: vec2(1.0, 0.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(1.0, 1.0, 0.0), // top-right
                    uv: vec2(1.0, 1.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(0.0, 0.0, 0.0), // bottom-left
                    uv: vec2(0.0, 0.0),
                    normal,
                },
            ];

            mesh::create(vertices.to_vec())
        });

        mesh.draw()
    }
}
