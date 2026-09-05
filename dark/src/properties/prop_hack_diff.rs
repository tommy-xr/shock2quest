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

/// Dark's `P$ModifyDif` and `P$Modify2Di` - the terms a gun is modified on,
/// for the first and the second modification. Both are the same 12-byte
/// `sTechInfo` record `P$HackDiff` is, read against the Modify skill; a gun
/// that authors no second record cannot be taken past modification one.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropModifyDiff(pub PropHackDiff);

impl PropModifyDiff {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(PropHackDiff::read(reader, len))
    }
}

#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropModify2Diff(pub PropHackDiff);

impl PropModify2Diff {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(PropHackDiff::read(reader, len))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{PropHackDiff, PropModify2Diff, PropModifyDiff, PropRepairDiff};

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

    /// The shipped pistol's `P$ModifyDif` and `P$Modify2Di` bytes (gamesys
    /// template -17): the first modification is a 40% base success chance
    /// against two mines, the second a harder 30% against four, both twenty
    /// nanites an attempt.
    #[test]
    fn parses_the_shipped_pistol_modify_difficulties() {
        let first = [
            0x28, 0x00, 0x00, 0x00, // success chance 40
            0x02, 0x00, 0x00, 0x00, // critical chance 2
            0x00, 0x00, 0xa0, 0x41, // cost 20.0
        ];
        let second = [
            0x1e, 0x00, 0x00, 0x00, // success chance 30
            0x04, 0x00, 0x00, 0x00, // critical chance 4
            0x00, 0x00, 0xa0, 0x41, // cost 20.0
        ];

        assert_eq!(
            PropModifyDiff::read(&mut Cursor::new(first), 12),
            PropModifyDiff(PropHackDiff {
                success_chance: 40,
                critical_chance: 2,
                cost: 20.0,
            })
        );
        assert_eq!(
            PropModify2Diff::read(&mut Cursor::new(second), 12),
            PropModify2Diff(PropHackDiff {
                success_chance: 30,
                critical_chance: 4,
                cost: 20.0,
            })
        );
    }
}
