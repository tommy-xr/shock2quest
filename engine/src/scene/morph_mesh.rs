//! Deformation uses an immutable base and a reusable GPU buffer. A shared model
//! may be drawn for several entities: each draw supplies its own weights, so
//! animating one instance never changes another instance's rest geometry.
use super::{
    Geometry, Vertex, VertexPositionTextureNormal, VertexPositionTextureSkinnedNormal, mesh,
};
use cgmath::{InnerSpace, Vector2, Vector3};
use std::cell::RefCell;

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
    fn deform(&mut self, deltas: &[Delta], weights: &[f32]);
}
macro_rules! morph_vertex {
    ($t:ty) => {
        impl MorphVertex for $t {
            fn deform(&mut self, deltas: &[Delta], weights: &[f32]) {
                for (delta, weight) in deltas.iter().zip(weights.iter().copied()) {
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
    deltas: Vec<Delta>,
    channels: usize,
    scratch: RefCell<Vec<T>>,
    last: RefCell<Vec<f32>>,
    names: Vec<String>,
    mesh: mesh::Mesh,
}
impl<T: MorphVertex> MorphMesh<T> {
    pub fn new(base: Vec<T>, deltas: Vec<[Delta; 3]>) -> Self {
        Self::build(base, deltas.into_iter().flatten().collect(), 3, Vec::new())
    }
    pub fn named(base: Vec<T>, deltas: Vec<Vec<Delta>>, names: Vec<String>) -> Self {
        assert_eq!(base.len(), deltas.len());
        assert!(
            deltas
                .iter()
                .all(|d| d.len() == names.len() || names.is_empty() && d.len() == 3)
        );
        let channels = names.len();
        Self::build(
            base,
            deltas.into_iter().flatten().collect(),
            channels,
            names,
        )
    }
    fn build(base: Vec<T>, deltas: Vec<Delta>, channels: usize, names: Vec<String>) -> Self {
        assert_eq!(deltas.len(), base.len() * channels);
        Self {
            mesh: mesh::create(base.clone()),
            scratch: RefCell::new(base.clone()),
            base,
            deltas,
            channels,
            last: RefCell::new(Vec::new()),
            names,
        }
    }
}
impl<T: MorphVertex> Geometry for MorphMesh<T> {
    fn has_morph(&self) -> bool {
        true
    }
    fn morph_names(&self) -> &[String] {
        &self.names
    }
    fn draw(&self) {
        self.draw_with_morph(&[]);
    }
    fn draw_with_morph(&self, weights: &[f32]) {
        if self.last.borrow().as_slice() != weights {
            let mut scratch = self.scratch.borrow_mut();
            scratch.clone_from(&self.base);
            for (vertex, deltas) in scratch
                .iter_mut()
                .zip(self.deltas.chunks(self.channels.max(1)))
            {
                vertex.deform(deltas, weights);
            }
            self.mesh.update(&scratch);
            let mut last = self.last.borrow_mut();
            last.clear();
            last.extend_from_slice(weights);
        }
        self.mesh.draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_channels_beyond_plant_slots_preserve_skinning_and_instance_bases() {
        let base = VertexPositionTextureSkinnedNormal {
            position: Vector3::new(1., 2., 3.),
            normal: Vector3::unit_y(),
            uv: Vector2::new(0., 0.),
            bone_indices: [3, 4, 0, 0],
            bone_weights: [0.75, 0.25, 0., 0.],
        };
        let mut deltas = vec![Delta::default(); 11];
        deltas[3].position = Vector3::unit_x();
        deltas[10].position = Vector3::unit_z();
        let mut weights = vec![0.; 11];
        weights[3] = 0.25;
        weights[10] = 0.75;
        let mut first = base.clone();
        first.deform(&deltas, &weights);
        let mut second = base.clone();
        weights[3] = -0.5;
        weights[10] = 0.;
        second.deform(&deltas, &weights);
        assert_eq!(first.position, Vector3::new(1.25, 2., 3.75));
        assert_eq!(second.position, Vector3::new(0.5, 2., 3.));
        assert_eq!(base.position, Vector3::new(1., 2., 3.));
        assert_eq!(first.bone_indices, base.bone_indices);
        assert_eq!(first.bone_weights, base.bone_weights);
    }

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
        a.deform(&[delta; 3], &[0.5, 0.0, 0.0]);
        b.deform(&[delta; 3], &[-0.5, 0.0, 0.0]);
        assert_eq!(a.position.x, 2.0);
        assert_eq!(b.position.x, 0.0);
        assert_eq!(base.position.x, 1.0);
        assert_eq!(a.uv.x, 0.5);
        assert!((a.normal.magnitude() - 1.0).abs() < 1e-6);
    }
}
