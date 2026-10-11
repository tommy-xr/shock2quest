use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::read_i32;

/// Dark's `sStatsDesc`: Strength, Endurance, Psionics, Agility, Cyber
/// affinity. Shipped data authors `BaseStats` on The Player archetype and on
/// mission start markers, whose loadout seeds a newly created player.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropBaseStats(pub [i32; 5]);

impl PropBaseStats {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 20, "a stats record contains five i32 values");
        Self(std::array::from_fn(|_| read_i32(reader)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// earth.mis's start marker: PSI 5 for the OSA training booth.
    #[test]
    fn stats_preserve_the_retail_field_order() {
        let mut bytes = Vec::new();
        for value in [1_i32, 1, 5, 1, 1] {
            bytes.extend(value.to_le_bytes());
        }
        let mut reader = Cursor::new(bytes);
        assert_eq!(PropBaseStats::read(&mut reader, 20).0, [1, 1, 5, 1, 1]);
        assert_eq!(reader.position(), 20);
    }
}
