use crate::{SCALE_FACTOR, ss2_common::read_single};
use serde::{Deserialize, Serialize};
use shipyard::Component;
use std::io::{Read, Seek};

#[derive(Clone, Debug, Component, Serialize, Deserialize)]
pub struct PropAISwarm {
    pub close_distance: f32,
    pub backoff_distance: f32,
}
impl Default for PropAISwarm {
    fn default() -> Self {
        Self {
            close_distance: 1.0 / SCALE_FACTOR,
            backoff_distance: 10.0 / SCALE_FACTOR,
        }
    }
}
impl PropAISwarm {
    pub fn read<T: Read + Seek>(reader: &mut T, _: u32) -> Self {
        Self {
            close_distance: read_single(reader) / SCALE_FACTOR,
            backoff_distance: read_single(reader) / SCALE_FACTOR,
        }
    }
}
#[derive(Clone, Debug, Component, Serialize, Deserialize)]
pub struct PropAIMoveZOffset(pub f32);
impl PropAIMoveZOffset {
    pub fn read<T: Read + Seek>(reader: &mut T, _: u32) -> Self {
        Self(read_single(reader) / SCALE_FACTOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swarm_distances_and_hover_offset_convert_dark_units() {
        let mut bytes =
            std::io::Cursor::new([1.0f32.to_le_bytes(), 10.0f32.to_le_bytes()].concat());
        let prop = PropAISwarm::read(&mut bytes, 8);
        assert_eq!(prop.close_distance, 0.4);
        assert_eq!(prop.backoff_distance, 4.0);
        assert_eq!(
            PropAIMoveZOffset::read(&mut std::io::Cursor::new(3.0f32.to_le_bytes()), 4).0,
            1.2
        );
    }
}
