use std::io;

use shipyard::Component;

use crate::ss2_common::*;
use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct FrobFlag: u32 {
        const Move = 1 << 0;
        const Script = 1 << 1;
        const Delete = 1 << 2;
        const Ignore = 1 << 3;
        const Focus = 1 << 4;
        const Tool = 1 << 5;
        const UseAmmo = 1 << 6;
        const Default = 1 << 7;
        const Deselect  = 1 << 8;
    }
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropFrobInfo {
    pub world_action: FrobFlag,
    pub inventory_action: FrobFlag,
    pub tool_action: FrobFlag,
}

impl PropFrobInfo {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropFrobInfo {
        let world_action = read_u32(reader);
        let inventory_action = read_u32(reader);
        let tool_action = read_u32(reader);
        let _zero = read_u32(reader);
        assert_eq!(_zero, 0);

        PropFrobInfo {
            world_action: FrobFlag::from_bits(world_action).unwrap(),
            inventory_action: FrobFlag::from_bits(inventory_action).unwrap(),
            tool_action: FrobFlag::from_bits(tool_action).unwrap(),
        }
    }
}
