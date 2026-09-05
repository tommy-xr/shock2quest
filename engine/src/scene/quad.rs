extern crate gl;
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

pub use crate::scene::Geometry;

use super::{Mesh, VertexPositionTextureNormal, mesh};
use cgmath::Vector2;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
/// Like [`Quad`] the mesh is built on first draw and cached (keyed by the UV
/// rectangle): a panel rebuilds its scene objects every frame, and allocating a
/// VAO and two VBOs per frame per element is exactly what that cache exists to
/// avoid.
pub struct QuadUv {
    uv_min: Vector2<f32>,
    uv_max: Vector2<f32>,
}

thread_local! {
    static UV_QUAD_GEOMETRY: RefCell<HashMap<[u32; 4], Rc<Mesh>>> =
        RefCell::new(HashMap::new());
}

pub fn create_with_uv(uv_min: Vector2<f32>, uv_max: Vector2<f32>) -> QuadUv {
    QuadUv { uv_min, uv_max }
}

impl QuadUv {
    fn mesh(&self) -> Rc<Mesh> {
        let key = [
            self.uv_min.x.to_bits(),
            self.uv_min.y.to_bits(),
            self.uv_max.x.to_bits(),
            self.uv_max.y.to_bits(),
        ];
        UV_QUAD_GEOMETRY.with(|cache| {
            cache
                .borrow_mut()
                .entry(key)
                .or_insert_with(|| Rc::new(self.build()))
                .clone()
        })
    }

    fn build(&self) -> Mesh {
        let normal = vec3(0.0, 0.0, 1.0);
        // Same corners as `Quad`, so an element's placement is unaffected by
        // whether it samples a sub-rectangle.
        let corner = |x: f32, y: f32| VertexPositionTextureNormal {
            position: vec3(x - 0.5, y - 0.5, 0.0),
            uv: vec2(
                self.uv_min.x + (self.uv_max.x - self.uv_min.x) * x,
                self.uv_min.y + (self.uv_max.y - self.uv_min.y) * y,
            ),
            normal,
        };
        mesh::create(vec![
            corner(0.0, 0.0),
            corner(0.0, 1.0),
            corner(1.0, 1.0),
            corner(1.0, 0.0),
            corner(1.0, 1.0),
            corner(0.0, 0.0),
        ])
    }
}

impl Geometry for QuadUv {
    fn draw(&self) {
        self.mesh().draw();
    }
}
