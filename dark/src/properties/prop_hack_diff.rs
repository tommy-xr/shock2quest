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
        assert_eq!(
            len, 12,
            "a tech difficulty must be a 12-byte sTechInfo record"
        );
        Self {
            success_chance: read_i32(reader),
            critical_chance: read_i32(reader),
            cost: read_single(reader),
        }
    }
}

/// Dark's `P$RepairDif` - the terms a broken object is repaired on. The same
/// 12-byte `sTechInfo` record as `P$HackDiff`, read against the Repair skill
/// rather than the Hack one, so the board can charge and roll for a repair the
/// way it does for a hack.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropRepairDiff(pub PropHackDiff);

impl PropRepairDiff {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(PropHackDiff::read(reader, len))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{PropHackDiff, PropRepairDiff};

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

    /// The shipped pistol's `P$RepairDif` bytes (gamesys template -17): a 20%
    /// base success chance, four criticals, three nanites an attempt.
    #[test]
    fn parses_the_shipped_pistol_repair_difficulty() {
        let bytes = [
            0x14, 0x00, 0x00, 0x00, // success chance 20
            0x04, 0x00, 0x00, 0x00, // critical chance 4
            0x00, 0x00, 0x40, 0x40, // cost 3.0
        ];

        assert_eq!(
            PropRepairDiff::read(&mut Cursor::new(bytes), 12),
            PropRepairDiff(PropHackDiff {
                success_chance: 20,
                critical_chance: 4,
                cost: 3.0,
            })
        );
    }
}
