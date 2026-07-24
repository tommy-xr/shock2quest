use serde::{Deserialize, Serialize};
use shipyard::Component;
use std::io;

use crate::ss2_common::{read_i32, read_single};

/// Dark's 12-byte `P$HackDiff` / `sTechInfo` record.
///
/// `success_chance` and `critical_chance` are the authored base values used by
/// the HRM board; `cost` is the nanite cost factor (the retail hack operation's
/// base cost is one, so the factor is also the displayed cost).
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropHackDiff {
    pub success_chance: i32,
    pub critical_chance: i32,
    pub cost: f32,
}

impl PropHackDiff {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 12, "P$HackDiff must be a 12-byte sTechInfo record");
        Self {
            success_chance: read_i32(reader),
            critical_chance: read_i32(reader),
            cost: read_single(reader),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::PropHackDiff;

    #[test]
    fn parses_little_endian_tech_info_layout() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&75i32.to_le_bytes());
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&15.5f32.to_le_bytes());

        let property = PropHackDiff::read(&mut Cursor::new(bytes), 12);
        assert_eq!(
            property,
            PropHackDiff {
                success_chance: 75,
                critical_chance: 2,
                cost: 15.5,
            }
        );
    }
}
