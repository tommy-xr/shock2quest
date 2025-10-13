use std::io;

use shipyard::Component;

use crate::ss2_common::read_u32;
use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct PhysicsModelType: u32 {
        const ORIENTED_BOUNDING_BOX = 0;
        const SPHERE = 1;
        const UNKNOWN = 2;
        const NONE = 3;
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPhysType {
    pub phys_type: PhysicsModelType, // TODO: Variant
    pub num_submodels: u32,
    pub remove_on_sleep: bool,
    pub is_special: bool,
}

impl PropPhysType {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPhysType {
        let phys_type_bits = read_u32(reader);
        let num_submodels = read_u32(reader);
        let remove_on_sleep = read_u32(reader) != 0;
        let is_special = read_u32(reader) != 0;

        PropPhysType {
            phys_type: PhysicsModelType::from_bits(phys_type_bits)
                .unwrap_or(PhysicsModelType::ORIENTED_BOUNDING_BOX),
            num_submodels,
            remove_on_sleep,
            is_special,
        }
    }
}
