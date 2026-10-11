//! Deformation uses an immutable base and a reusable GPU buffer. A shared model
//! may be drawn for several entities: each draw supplies its own weights, so
//! animating one instance never changes another instance's rest geometry.
use super::{
    Geometry, Vertex, VertexPositionTextureNormal, VertexPositionTextureSkinnedNormal, mesh,
};
use cgmath::{InnerSpace, Vector2, Vector3};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug)]
pub struct Delta {
    pub position: Vector3<f32>,
    pub normal: Vector3<f32>,
    pub uv: Vector2<f32>,
}
impl Default for Delta {
    fn default() -> Self {
        Self {
            position: Vector3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 0.0),
            uv: Vector2::new(0.0, 0.0),
        }
    }
}
pub trait MorphVertex: Vertex + Clone {
    fn deform(&mut self, deltas: &[Delta; 3], weights: [f32; 3]);
}
macro_rules! morph_vertex {
    ($t:ty) => {
        impl MorphVertex for $t {
            fn deform(&mut self, deltas: &[Delta; 3], weights: [f32; 3]) {
                for (delta, weight) in deltas.iter().zip(weights) {
                    self.position += delta.position * weight;
                    self.normal += delta.normal * weight;
                    self.uv += delta.uv * weight;
                }
                if self.normal.magnitude2() > 0.0 {
                    self.normal = self.normal.normalize();
                }
            }
        }
    };
}
morph_vertex!(VertexPositionTextureNormal);
morph_vertex!(VertexPositionTextureSkinnedNormal);

pub struct MorphMesh<T: MorphVertex> {
    base: Vec<T>,
    deltas: Vec<[Delta; 3]>,
    scratch: RefCell<Vec<T>>,
    last: Cell<[f32; 3]>,
    mesh: mesh::Mesh,
}
impl<T: MorphVertex> MorphMesh<T> {
    pub fn new(base: Vec<T>, deltas: Vec<[Delta; 3]>) -> Self {
        assert_eq!(base.len(), deltas.len());
        Self {
            mesh: mesh::create(base.clone()),
            scratch: RefCell::new(base.clone()),
            base,
            deltas,
            last: Cell::new([0.0; 3]),
        }
    }
}
impl<T: MorphVertex> Geometry for MorphMesh<T> {
    fn has_morph(&self) -> bool {
        true
    }
    fn draw(&self) {
        self.draw_with_morph([0.0; 3]);
    }
    fn draw_with_morph(&self, weights: [f32; 3]) {
        if self.last.get() != weights {
            let mut scratch = self.scratch.borrow_mut();
            scratch.clone_from(&self.base);
            for (vertex, deltas) in scratch.iter_mut().zip(&self.deltas) {
                vertex.deform(deltas, weights);
            }
            self.mesh.update(&scratch);
            self.last.set(weights);
        }
        self.mesh.draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deformation_is_additive_signed_and_does_not_mutate_the_base() {
        let base = VertexPositionTextureNormal {
            position: Vector3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            uv: Vector2::new(0.0, 0.0),
        };
        let delta = Delta {
            position: Vector3::new(2.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            uv: Vector2::new(1.0, 0.0),
        };
        let mut a = base.clone();
        let mut b = base.clone();
        a.deform(&[delta; 3], [0.5, 0.0, 0.0]);
        b.deform(&[delta; 3], [-0.5, 0.0, 0.0]);
        assert_eq!(a.position.x, 2.0);
        assert_eq!(b.position.x, 0.0);
        assert_eq!(base.position.x, 1.0);
        assert_eq!(a.uv.x, 0.5);
        assert!((a.normal.magnitude() - 1.0).abs() < 1e-6);
    }
}
