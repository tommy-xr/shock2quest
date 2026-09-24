use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_u8, read_u16};

/// Dark's `sSchemaLoopParams` (`P$SchLoopPa`) record: how a schema repeats.
///
/// Zero intervals loop one sample seamlessly; otherwise a sample re-triggers
/// every random `interval_min..=interval_max` ms.
#[derive(Clone, Debug, Component, Default, Deserialize, PartialEq, Serialize)]
pub struct PropSchemaLoopParams {
    pub flags: u8,
    pub max_samples: u8,
    /// Repeat limit, honored only with [`Self::COUNT`].
    pub count: u16,
    pub interval_min: u16,
    pub interval_max: u16,
}

impl PropSchemaLoopParams {
    pub const POLY: u8 = 1 << 0;
    pub const COUNT: u8 = 1 << 1;

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        Self {
            flags: read_u8(reader),
            max_samples: read_u8(reader),
            count: read_u16(reader),
            interval_min: read_u16(reader),
            interval_max: read_u16(reader),
        }
    }

    /// One sample repeated back to back, forever (e.g. the alarm klaxon).
    pub fn is_seamless(&self) -> bool {
        self.flags & Self::COUNT == 0 && self.interval_min == 0 && self.interval_max == 0
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_klaxon_record_as_seamless() {
        // klaxalarm: no flags, one sample, zero intervals.
        let bytes = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let parsed = PropSchemaLoopParams::read(&mut Cursor::new(bytes), 8);
        assert_eq!(
            parsed,
            PropSchemaLoopParams {
                max_samples: 1,
                ..Default::default()
            }
        );
        assert!(parsed.is_seamless());
    }

    #[test]
    fn interval_or_count_loops_are_not_seamless() {
        // flags COUNT, count 3, interval 500..=1500 ms.
        let bytes = [0x02, 0x01, 0x03, 0x00, 0xf4, 0x01, 0xdc, 0x05];
        let parsed = PropSchemaLoopParams::read(&mut Cursor::new(bytes), 8);
        assert_eq!(
            parsed,
            PropSchemaLoopParams {
                flags: PropSchemaLoopParams::COUNT,
                max_samples: 1,
                count: 3,
                interval_min: 500,
                interval_max: 1500,
            }
        );
        assert!(!parsed.is_seamless());
        let count_only = PropSchemaLoopParams {
            flags: PropSchemaLoopParams::COUNT,
            count: 3,
            ..Default::default()
        };
        assert!(!count_only.is_seamless());
    }
}
