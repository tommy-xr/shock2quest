use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_single};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAICamera {
    pub scan_angle_1: f32,
    pub scan_angle_2: f32,
    pub scan_speed: f32,
}

impl PropAICamera {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAICamera {
        let scan_angle_1 = read_single(reader);
        let scan_angle_2 = read_single(reader);
        let scan_speed = read_single(reader);

        const EXPECTED_SIZE: u32 = 12;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAICamera {
            scan_angle_1,
            scan_angle_2,
            scan_speed,
        }
    }
}
