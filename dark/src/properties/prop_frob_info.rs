use std::io;

use shipyard::Component;

use crate::ss2_common::*;
use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct FrobFlag: u32 {
        const MOVE = 1 << 0;
        const SCRIPT = 1 << 1;
        const DELETE = 1 << 2;
        const IGNORE = 1 << 3;
        const FOCUS = 1 << 4;
        const TOOL = 1 << 5;
        const USE_AMMO = 1 << 6;
        const DEFAULT = 1 << 7;
        const DESELECT  = 1 << 8;
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
