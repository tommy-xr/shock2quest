use std::io;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size, read_u32};

bitflags! {
    /// Retail `eSpawnFlags` (`P$Spawn.Flags`).
    #[derive(Serialize, Deserialize)]
    pub struct SpawnFlags: u32 {
        const POP_LIMIT = 0x01;
        const PLAYER_DISTANCE = 0x02;
        const GOTO_LOCATION = 0x04;
        const SELF_MARKER = 0x08;
        const RAYCAST = 0x10;
        const FARTHEST = 0x20;
    }
}

/// Retail `sSpawnInfo` (`P$Spawn`).
#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropSpawn {
    pub object_names: [String; 4],
    pub odds: [i32; 4],
    pub flags: SpawnFlags,
    /// `0` is unlimited, `-1` is exhausted.
    pub supply: i32,
}

impl PropSpawn {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        let object_names = std::array::from_fn(|_| read_string_with_size(reader, 64));
        let odds = std::array::from_fn(|_| read_i32(reader));
        let flags = SpawnFlags::from_bits_truncate(read_u32(reader));
        let supply = read_i32(reader);
        Self {
            object_names,
            odds,
            flags,
            supply,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_retail_spawn_layout() {
        let mut bytes = Vec::new();
        for name in ["SHODAN", "", "", ""] {
            let mut field = name.as_bytes().to_vec();
            field.resize(64, 0);
            bytes.extend_from_slice(&field);
        }
        for odds in [100i32, 0, 0, 0] {
            bytes.extend_from_slice(&odds.to_le_bytes());
        }
        bytes.extend_from_slice(
            &(SpawnFlags::POP_LIMIT | SpawnFlags::GOTO_LOCATION | SpawnFlags::RAYCAST)
                .bits()
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(bytes.len(), 280);

        let spawn = PropSpawn::read(&mut Cursor::new(bytes), 280);
        assert_eq!(spawn.object_names[0], "SHODAN");
        assert_eq!(spawn.odds, [100, 0, 0, 0]);
        assert_eq!(
            spawn.flags,
            SpawnFlags::POP_LIMIT | SpawnFlags::GOTO_LOCATION | SpawnFlags::RAYCAST
        );
        assert_eq!(spawn.supply, 0);
    }
}
