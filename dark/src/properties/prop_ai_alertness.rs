use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use super::AIAlertLevel;
use crate::ss2_common::{read_bytes, read_u32};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIAlertness {
    pub level: AIAlertLevel,
    pub peak: AIAlertLevel,
}

impl PropAIAlertness {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIAlertness {
        let level = AIAlertLevel::from_raw(read_u32(reader));
        let peak = AIAlertLevel::from_raw(read_u32(reader));

        const EXPECTED_SIZE: u32 = 8;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIAlertness { level, peak }
    }
}
