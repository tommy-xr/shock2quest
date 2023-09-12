use std::io;

use shipyard::Component;

use crate::ss2_common::read_u32;

use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct CollisionType: u32 {
        const BOUNCE = 1 << 0;
        const DESTROY_ON_IMPACT = 1 << 1;
        const SLAY_ON_IMPACT = 1 << 2;
        const NO_COLLISION_SOUND = 1 << 3;
        const NO_RESULT = 1 << 4;
        const FULL_COLLISION_SOUND = 1 << 5;
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropCollisionType {
    pub collision_type: CollisionType,
}

impl PropCollisionType {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropCollisionType {
        let collision_type_u32 = read_u32(reader);
        let collision_type = CollisionType::from_bits(collision_type_u32).unwrap();
        PropCollisionType { collision_type }
    }
}
