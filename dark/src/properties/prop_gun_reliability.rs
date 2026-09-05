use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::read_single;

use serde::{Deserialize, Serialize};

/// `P$GunReliab` - how a gun wears and how likely it is to break, in on-disk
/// field order. Authored on the gun templates (the pistol reads
/// 0.5 / 5.0 / 1.0 / 10.0); guns without the property never wear.
///
/// Records come in two lengths: gamesys templates author 20 bytes (a trailing
/// float that is 0.0 on every shipped gun), mission overrides 16.
/// [`PropGunReliability::read`] takes each field only if `len` covers it, then
/// snaps to the end of the chunk, so both parse.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropGunReliability {
    /// Break chance, in percent, at `thresh_break` condition.
    pub min_break: f32,
    /// Break chance, in percent, at zero condition.
    pub max_break: f32,
    /// Condition points lost per shot fired.
    pub degrade_rate: f32,
    /// Condition above which the gun cannot break.
    pub thresh_break: f32,
}

impl PropGunReliability {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropGunReliability {
        let start = reader.stream_position().unwrap();
        let min_break = if len >= 4 { read_single(reader) } else { 0.0 };
        let max_break = if len >= 8 { read_single(reader) } else { 0.0 };
        let degrade_rate = if len >= 12 { read_single(reader) } else { 0.0 };
        let thresh_break = if len >= 16 { read_single(reader) } else { 0.0 };
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
        PropGunReliability {
            min_break,
            max_break,
            degrade_rate,
            thresh_break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(hex: &str) -> PropGunReliability {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let mut cursor = Cursor::new(bytes.clone());
        let prop = PropGunReliability::read(&mut cursor, bytes.len() as u32);
        // The reader must land exactly on the end of the record.
        assert_eq!(cursor.position(), bytes.len() as u64);
        prop
    }

    /// The shipped 20-byte gamesys record for the pistol.
    #[test]
    fn parses_the_twenty_byte_gamesys_record() {
        let prop = read("0000003f0000a0400000803f0000204100000000");
        assert_eq!(prop.min_break, 0.5);
        assert_eq!(prop.max_break, 5.0);
        assert_eq!(prop.degrade_rate, 1.0);
        assert_eq!(prop.thresh_break, 10.0);
    }

    /// A 16-byte mission override (no trailing float).
    #[test]
    fn parses_the_sixteen_byte_mission_record() {
        let prop = read("0000003f0000a0400000803f00002041");
        assert_eq!(prop.min_break, 0.5);
        assert_eq!(prop.max_break, 5.0);
        assert_eq!(prop.degrade_rate, 1.0);
        assert_eq!(prop.thresh_break, 10.0);
    }

    /// The exotic Crystal Shard: always-breaking, heavy wear.
    #[test]
    fn parses_the_crystal_shard_record() {
        let prop = read("0000be420000be42000020410000c84200000000");
        assert_eq!(prop.min_break, 95.0);
        assert_eq!(prop.max_break, 95.0);
        assert_eq!(prop.degrade_rate, 10.0);
        assert_eq!(prop.thresh_break, 100.0);
    }
}
