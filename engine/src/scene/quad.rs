extern crate gl;
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

pub use crate::scene::Geometry;

use super::{Mesh, VertexPositionTextureNormal, mesh};
use cgmath::Vector2;

pub struct Quad;

static QUAD_GEOMETRY: OnceCell<Mesh> = OnceCell::new();

pub fn create() -> Quad {
    Quad
}

impl Geometry for Quad {
    fn draw(&self) {
        let mesh = QUAD_GEOMETRY.get_or_init(|| {
            // Normal pointing forward (positive Z direction)
            let normal = vec3(0.0, 0.0, 1.0);

            let vertices: [VertexPositionTextureNormal; 6] = [
                // Tri 1
                VertexPositionTextureNormal {
                    position: vec3(-0.5, -0.5, 0.0),
                    uv: vec2(0.0, 0.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(-0.5, 0.5, 0.0),
                    uv: vec2(0.0, 1.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(0.5, 0.5, 0.0),
                    uv: vec2(1.0, 1.0),
                    normal,
                },
                // Tri2
                VertexPositionTextureNormal {
                    position: vec3(0.5, -0.5, 0.0),
                    uv: vec2(1.0, 0.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(0.5, 0.5, 0.0),
                    uv: vec2(1.0, 1.0),
                    normal,
                },
                VertexPositionTextureNormal {
                    position: vec3(-0.5, -0.5, 0.0),
                    uv: vec2(0.0, 0.0),
                    normal,
                },
            ];

            mesh::create(vertices.to_vec())
        });

        mesh.draw();
    }
}

/// A [`Quad`] that samples an arbitrary rectangle of its texture instead of the
/// whole of it. `uv_min`/`uv_max` are in texture units, so a range wider than
/// 0..1 tiles the texture (with `TextureOptions::wrap`) - which is how a grid
/// panel repeats one cell bitmap across its cells.
///
/// Unlike [`Quad`] the geometry is per-instance (the UVs vary), so this owns
/// its mesh rather than sharing the cached one.
pub struct QuadUv {
    mesh: Mesh,
}

pub fn create_with_uv(uv_min: Vector2<f32>, uv_max: Vector2<f32>) -> QuadUv {
    let normal = vec3(0.0, 0.0, 1.0);
    // Same corners as `Quad`, so an element's placement is unaffected by
    // whether it samples a sub-rectangle.
    let corner = |x: f32, y: f32| VertexPositionTextureNormal {
        position: vec3(x - 0.5, y - 0.5, 0.0),
        uv: vec2(
            uv_min.x + (uv_max.x - uv_min.x) * x,
            uv_min.y + (uv_max.y - uv_min.y) * y,
        ),
        normal,
    };
    let vertices = vec![
        corner(0.0, 0.0),
        corner(0.0, 1.0),
        corner(1.0, 1.0),
        corner(1.0, 0.0),
        corner(1.0, 1.0),
        corner(0.0, 0.0),
    ];
    QuadUv {
        mesh: mesh::create(vertices),
    }
}

impl Geometry for QuadUv {
    fn draw(&self) {
        self.mesh.draw();
    }
}
