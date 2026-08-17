use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_i32, read_u32};

/// Dark's `sSchemaPlayParams` (`P$SchPlayPa`) record.
///
/// Volumes and pans use the sound library's millibel scale. Volume runs from
/// -10_000 (silent) to -1 (nominal); pan runs from -10_000 (left) through 0
/// (center) to 10_000 (right).
#[derive(Clone, Debug, Component, Deserialize, PartialEq, Serialize)]
pub struct PropSchemaPlayParams {
    pub flags: u32,
    pub volume: i32,
    pub pan: i32,
    pub initial_delay: i32,
    pub fade: i32,
}

impl Default for PropSchemaPlayParams {
    fn default() -> Self {
        Self {
            flags: 0,
            volume: -1,
            pan: 0,
            initial_delay: 0,
            fade: 0,
        }
    }
}

impl PropSchemaPlayParams {
    pub const PAN_POSITION: u32 = 1 << 1;
    pub const PAN_RANGE: u32 = 1 << 2;

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        Self {
            flags: read_u32(reader),
            volume: read_i32(reader),
            pan: read_i32(reader),
            initial_delay: read_i32(reader),
            fade: read_i32(reader),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_all_five_little_endian_words() {
        let bytes = [
            0x02, 0x00, 0x01, 0x00, // flags = 0x00010002
            0x18, 0xfc, 0xff, 0xff, // volume = -1000
            0x44, 0xfd, 0xff, 0xff, // pan = -700
            0xc8, 0x00, 0x00, 0x00, // delay = 200
            0x2c, 0x01, 0x00, 0x00, // fade = 300
        ];
        let parsed = PropSchemaPlayParams::read(&mut Cursor::new(bytes), bytes.len() as u32);
        assert_eq!(
            parsed,
            PropSchemaPlayParams {
                flags: 0x0001_0002,
                volume: -1000,
                pan: -700,
                initial_delay: 200,
                fade: 300,
            }
        );
    }
}
