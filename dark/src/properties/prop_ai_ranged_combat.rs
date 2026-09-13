use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single};

use serde::{Deserialize, Serialize};

/// `P$AIRCProp` - how a ranged AI wants to stand off from its target, in
/// on-disk field order (7 four-byte fields, 28 bytes).
///
/// Distances are stored as Dark-unit integers. They score candidate standing
/// positions; the minimum is not a prohibition on firing at a nearby target.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropAIRangedCombat {
    /// Closest desirable standing distance, in Dark units.
    pub minimum_distance: i32,
    /// The range the AI manoeuvres toward.
    pub ideal_distance: i32,
    /// Seconds it waits before loosing a shot.
    pub firing_delay: f32,
    /// How strongly it prefers to shoot from cover.
    pub cover_desire: i32,
    pub decay_speed: f32,
    /// Authored moving-fire preference (0..5), not a boolean.
    pub fire_while_moving: i32,
    pub contain_projectile: i32,
}

impl PropAIRangedCombat {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIRangedCombat {
        // Take each field only if the record is long enough to hold it, then
        // snap to the end of the chunk, so a short mission override parses.
        let start = reader.stream_position().unwrap();
        let minimum_distance = if len >= 4 { read_i32(reader) } else { 0 };
        let ideal_distance = if len >= 8 { read_i32(reader) } else { 0 };
        let firing_delay = if len >= 12 { read_single(reader) } else { 0.0 };
        let cover_desire = if len >= 16 { read_i32(reader) } else { 0 };
        let decay_speed = if len >= 20 { read_single(reader) } else { 0.0 };
        let fire_while_moving = if len >= 24 { read_i32(reader) } else { 0 };
        let contain_projectile = if len >= 28 { read_i32(reader) } else { 0 };
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
        PropAIRangedCombat {
            minimum_distance,
            ideal_distance,
            firing_delay,
            cover_desire,
            decay_speed,
            fire_while_moving,
            contain_projectile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(hex: &str) -> PropAIRangedCombat {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let mut cursor = Cursor::new(bytes.clone());
        let prop = PropAIRangedCombat::read(&mut cursor, bytes.len() as u32);
        // The reader must land exactly on the end of the record.
        assert_eq!(cursor.position(), bytes.len() as u64);
        prop
    }

    /// A full 28-byte record: distances are plain ints, the delays floats.
    #[test]
    fn parses_a_full_record() {
        let prop = read("0a000000280000000000004003000000000080400100000000000000");
        assert_eq!(prop.minimum_distance, 10);
        assert_eq!(prop.ideal_distance, 40);
        assert_eq!(prop.firing_delay, 2.0);
        assert_eq!(prop.cover_desire, 3);
        assert_eq!(prop.decay_speed, 4.0);
        assert_eq!(prop.fire_while_moving, 1);
    }

    /// A record too short for the tail still parses, leaving the rest zeroed.
    #[test]
    fn parses_a_truncated_record() {
        let prop = read("0a00000028000000");
        assert_eq!(prop.minimum_distance, 10);
        assert_eq!(prop.ideal_distance, 40);
        assert_eq!(prop.firing_delay, 0.0);
    }
}

/// `P$AIRCRange`: boundaries between very-short, short, ideal, long and
/// very-long range. Runtime units, like other parsed AI movement distances.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropAIRangedRanges(pub [f32; 4]);
impl Default for PropAIRangedRanges {
    fn default() -> Self {
        Self([
            0.0,
            10.0 / crate::SCALE_FACTOR,
            30.0 / crate::SCALE_FACTOR,
            f32::MAX / crate::SCALE_FACTOR,
        ])
    }
}
impl PropAIRangedRanges {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        let start = reader.stream_position().unwrap();
        let mut ranges = Self::default();
        for (i, range) in ranges.0.iter_mut().enumerate() {
            if len >= (i as u32 + 1) * 4 {
                *range = read_single(reader) / crate::SCALE_FACTOR;
            }
        }
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
        ranges
    }
}

#[cfg(test)]
mod range_tests {
    use super::*;
    #[test]
    fn authored_range_bands_convert_once_and_consume_the_record() {
        let bytes: Vec<_> = [5.0f32, 10.0, 15.0, 30.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut input = std::io::Cursor::new(bytes);
        assert_eq!(
            PropAIRangedRanges::read(&mut input, 16).0,
            [2.0, 4.0, 6.0, 12.0]
        );
        assert_eq!(input.position(), 16);
    }
    #[test]
    fn short_range_records_retain_defaults_for_absent_fields() {
        let mut input = std::io::Cursor::new(5.0f32.to_le_bytes());
        let range = PropAIRangedRanges::read(&mut input, 4);
        assert_eq!(range.0[0], 2.0);
        assert_eq!(range.0[1..], PropAIRangedRanges::default().0[1..]);
    }
}
