use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_u32};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIAwareDelay {
    pub to_two: u32,
    pub to_three: u32,
    pub two_reuse: u32,
    pub three_reuse: u32,
    pub ignore_range: u32,
}

impl PropAIAwareDelay {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIAwareDelay {
        let to_two = read_u32(reader);
        let to_three = read_u32(reader);
        let two_reuse = read_u32(reader);
        let three_reuse = read_u32(reader);
        let ignore_range = read_u32(reader);

        const EXPECTED_SIZE: u32 = 20;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIAwareDelay {
            to_two,
            to_three,
            two_reuse,
            three_reuse,
            ignore_range,
        }
    }
}
