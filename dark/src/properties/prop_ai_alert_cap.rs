use std::io;

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_u32};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromPrimitive, ToPrimitive)]
pub enum AIAlertLevel {
    Lowest = 0,
    Low = 1,
    Moderate = 2,
    High = 3,
}

impl AIAlertLevel {
    pub fn from_raw(raw: u32) -> AIAlertLevel {
        AIAlertLevel::from_u32(raw).unwrap_or(AIAlertLevel::High)
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIAlertCap {
    pub max_level: AIAlertLevel,
    pub min_level: AIAlertLevel,
    pub min_relax: AIAlertLevel,
}

impl PropAIAlertCap {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIAlertCap {
        let max_level = AIAlertLevel::from_raw(read_u32(reader));
        let min_level = AIAlertLevel::from_raw(read_u32(reader));
        let min_relax = AIAlertLevel::from_raw(read_u32(reader));

        const EXPECTED_SIZE: u32 = 12;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIAlertCap {
            max_level,
            min_level,
            min_relax,
        }
    }
}
