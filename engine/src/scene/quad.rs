extern crate gl;
use cgmath::{vec2, vec3};
use once_cell::sync::OnceCell;

use self::gl::types::*;
pub use crate::scene::Geometry;
use std::mem;
use std::os::raw::c_void;
use std::ptr;

use super::{mesh, Mesh, VertexPositionTexture};

pub struct Quad;

static QUAD_GEOMETRY: OnceCell<Mesh> = OnceCell::new();

pub fn create() -> Quad {
    Quad
}

impl Geometry for Quad {
    fn draw(&self) {
        let mesh = QUAD_GEOMETRY.get_or_init(|| {
            let vertices: [VertexPositionTexture; 6] = [
                // Tri 1
                VertexPositionTexture {
                    position: vec3(-0.5, -0.5, 0.0),
                    uv: vec2(0.0, 0.0),
                },
                VertexPositionTexture {
                    position: vec3(-0.5, 0.5, 0.0),
                    uv: vec2(0.0, 1.0),
                },
                VertexPositionTexture {
                    position: vec3(0.5, 0.5, 0.0),
                    uv: vec2(1.0, 1.0),
                },
                // Tri2
                VertexPositionTexture {
                    position: vec3(0.5, -0.5, 0.0),
                    uv: vec2(1.0, 0.0),
                },
                VertexPositionTexture {
                    position: vec3(0.5, 0.5, 0.0),
                    uv: vec2(1.0, 1.0),
                },
                VertexPositionTexture {
                    position: vec3(-0.5, -0.5, 0.0),
                    uv: vec2(0.0, 0.0),
                },
            ];

            mesh::create(vertices.to_vec())
        });

        mesh.draw();
    }
}
