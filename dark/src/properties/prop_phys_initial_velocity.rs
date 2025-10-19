use std::io;

use cgmath::Vector3;
use shipyard::Component;

use serde::{Deserialize, Serialize};

use crate::{ss2_common::read_vec3, SCALE_FACTOR};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPhysInitialVelocity(pub Vector3<f32>);

impl PropPhysInitialVelocity {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPhysInitialVelocity {
        let velocity = read_vec3(reader) / SCALE_FACTOR;

        PropPhysInitialVelocity(velocity)
    }
}
