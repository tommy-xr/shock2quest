use std::io;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_u32};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromPrimitive)]
pub enum AIMode {
    Asleep = 0,
    SuperEfficient = 1,
    Efficient = 2,
    Normal = 3,
    Combat = 4,
    Dead = 5,
}

impl AIMode {
    fn from_raw(raw: u32) -> AIMode {
        AIMode::from_u32(raw).unwrap_or(AIMode::Normal)
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIMode {
    pub mode: AIMode,
}

impl PropAIMode {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIMode {
        let mode = AIMode::from_raw(read_u32(reader));

        const EXPECTED_SIZE: u32 = 4;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIMode { mode }
    }
}
