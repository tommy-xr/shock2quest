use std::io;

use shipyard::Component;

use crate::ss2_common::read_u32;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHitPoints {
    pub hit_points: i32,
}

impl PropHitPoints {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropHitPoints {
        let hit_points = read_u32(reader) as i32;
        PropHitPoints { hit_points }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMaxHitPoints {
    pub hit_points: u32,
}

impl PropMaxHitPoints {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropMaxHitPoints {
        let hit_points = read_u32(reader);
        PropMaxHitPoints { hit_points }
    }
}
