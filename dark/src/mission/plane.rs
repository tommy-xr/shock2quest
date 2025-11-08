use std::io;

use cgmath::{InnerSpace, Vector3};

use crate::{
    SCALE_FACTOR,
    ss2_common::{read_single, read_vec3},
};

#[derive(Clone, Debug)]
pub struct Plane {
    pub normal: Vector3<f32>,
    pub w: f32,
}

impl Plane {
    pub fn read<T: io::Read>(reader: &mut T) -> Plane {
        let normal = read_vec3(reader).normalize();
        let w = read_single(reader) / SCALE_FACTOR;

        Plane { normal, w }
    }
}
