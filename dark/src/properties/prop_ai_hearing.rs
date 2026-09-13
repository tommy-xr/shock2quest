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
    /// Default Dark Engine AIHearStat distance multipliers (aibassns.cpp).
    /// Missing properties use Normal (3); clamp malformed ratings to VeryHigh.
    pub fn range_multiplier(&self) -> f32 {
        [0.0, 0.25, 0.65, 1.0, 1.5, 3.0][self.rating.min(5) as usize]
    }

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
