use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_u32};

/// AI hearing acuity rating (P$AI_Hearin), 0-5: 0 = deaf (the AI ignores
/// noises entirely - e.g. the `Deaf` metaproperty sets it), higher values are
/// progressively more acute. Creatures without the property hear normally.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIHearing {
    pub rating: u32,
}

impl PropAIHearing {
    pub fn is_deaf(&self) -> bool {
        self.rating == 0
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIHearing {
        let rating = read_u32(reader);

        const EXPECTED_SIZE: u32 = 4;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIHearing { rating }
    }
}
