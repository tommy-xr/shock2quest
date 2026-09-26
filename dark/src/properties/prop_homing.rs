use std::{io, time::Duration};

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::{
    SCALE_FACTOR,
    ss2_common::{read_single, read_u32},
};

/// Dark's `sHoming` (shkhome.h): two fix-angle limits and a millisecond pulse.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHoming {
    pub target_types: u32,
    pub distance: f32,
    pub heading_limit: f32,
    pub max_turn: f32,
    pub update_interval: Duration,
}

impl PropHoming {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        Self {
            target_types: read_u32(reader),
            distance: read_single(reader) / SCALE_FACTOR,
            heading_limit: read_u32(reader) as f32 * std::f32::consts::TAU / 65536.0,
            max_turn: read_u32(reader) as f32 * std::f32::consts::TAU / 65536.0,
            update_interval: Duration::from_millis(read_u32(reader) as u64),
        }
    }
}

/// Authored target-category bits; a target may belong to both categories.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTargetType(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shipped_annelid_homing_record() {
        let mut bytes = Vec::new();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(50f32.to_le_bytes());
        bytes.extend(4944u32.to_le_bytes());
        bytes.extend(2048u32.to_le_bytes());
        bytes.extend(200u32.to_le_bytes());
        let mut reader = io::Cursor::new(bytes);
        let prop = PropHoming::read(&mut reader, 20);
        assert_eq!(reader.position(), 20);
        assert_eq!(prop.target_types, 2);
        assert_eq!(prop.distance, 50.0 / SCALE_FACTOR);
        assert!((prop.heading_limit.to_degrees() - 27.158203).abs() < 0.001);
        assert!((prop.max_turn.to_degrees() - 11.25).abs() < 0.001);
        assert_eq!(prop.update_interval, Duration::from_millis(200));
    }
}
