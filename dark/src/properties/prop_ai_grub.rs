use crate::{SCALE_FACTOR, ss2_common::*};
use serde::{Deserialize, Serialize};
use shipyard::Component;
use std::io::{Read, Seek};

/// Dark's sAIGrubCombatParams (`P$AI_Grub_C`, 60 bytes).
#[derive(Clone, Debug, Component, Serialize, Deserialize)]
pub struct PropAIGrubCombat {
    pub leap_distance: f32,
    pub bite_distance: f32,
    pub stimulus: String,
    pub intensity: f32,
    pub horizontal_speed: f32,
    pub vertical_speed: f32,
    pub min_leap_ms: u32,
    pub max_leap_ms: u32,
}
impl PropAIGrubCombat {
    pub fn read<T: Read + Seek>(reader: &mut T, _len: u32) -> Self {
        Self {
            leap_distance: read_single(reader) / SCALE_FACTOR,
            bite_distance: read_single(reader) / SCALE_FACTOR,
            stimulus: read_string_with_size(reader, 32),
            intensity: read_single(reader),
            horizontal_speed: read_single(reader) / SCALE_FACTOR,
            vertical_speed: read_single(reader) / SCALE_FACTOR,
            min_leap_ms: read_u32(reader),
            max_leap_ms: read_u32(reader),
        }
    }
}

#[derive(Clone, Debug, Component, Serialize, Deserialize)]
pub struct PropAIMoveSpeed(pub f32);
impl PropAIMoveSpeed {
    pub fn read<T: Read + Seek>(reader: &mut T, _len: u32) -> Self {
        Self(read_single(reader) / SCALE_FACTOR)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_retail_grub_combat_without_inventing_bite_damage() {
        let mut bytes = Vec::new();
        bytes.extend(15.0f32.to_le_bytes());
        bytes.extend(0.0f32.to_le_bytes());
        bytes.extend([0u8; 32]);
        for value in [0.0f32, 12.0, 50.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        let mut reader = std::io::Cursor::new(bytes);
        let config = PropAIGrubCombat::read(&mut reader, 60);
        assert_eq!(reader.position(), 60);
        assert_eq!(config.leap_distance, 15.0 / SCALE_FACTOR);
        assert_eq!(config.horizontal_speed, 12.0 / SCALE_FACTOR);
        assert_eq!(config.vertical_speed, 50.0 / SCALE_FACTOR);
        assert_eq!(config.bite_distance, 0.0);
        assert_eq!(config.intensity, 0.0);
        assert!(config.stimulus.is_empty());
        assert_eq!((config.min_leap_ms, config.max_leap_ms), (0, 1));
    }
}
