use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_i32, read_single};

/// Retail `sEcologyInfo` (`P$Ecology`): population thresholds and polling
/// cadence for the three Dark ecology states (normal, hacked, alert).
#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropEcology {
    pub period_seconds: f32,
    pub min_count: [i32; 3],
    pub max_count: [i32; 3],
    pub recovery_seconds: [f32; 3],
    pub random_chance: [i32; 3],
}

impl PropEcology {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        let period_seconds = read_single(reader);
        let min_count = std::array::from_fn(|_| read_i32(reader));
        let max_count = std::array::from_fn(|_| read_i32(reader));
        let recovery_seconds = std::array::from_fn(|_| read_single(reader));
        let random_chance = std::array::from_fn(|_| read_i32(reader));
        Self {
            period_seconds,
            min_count,
            max_count,
            recovery_seconds,
            random_chance,
        }
    }
}

/// Ecology membership id. Spawn generators copy their value to every child.
#[derive(Debug, Component, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropEcoType(pub i32);

/// Current retail ecology state: 0 normal, 1 hacked, 2 alert.
#[derive(Debug, Component, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropEcoState(pub i32);

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_retail_ecology_layout() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&15.0f32.to_le_bytes());
        for value in [1i32, 0, 2, 1, 0, 3] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 120.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1i32, 0, 4] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(bytes.len(), 52);

        let ecology = PropEcology::read(&mut Cursor::new(bytes), 52);
        assert_eq!(ecology.period_seconds, 15.0);
        assert_eq!(ecology.min_count, [1, 0, 2]);
        assert_eq!(ecology.max_count, [1, 0, 3]);
        assert_eq!(ecology.recovery_seconds, [0.0, 0.0, 120.0]);
        assert_eq!(ecology.random_chance, [1, 0, 4]);
    }
}
