use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single};

use serde::{Deserialize, Serialize};

/// `P$GunState` - the mutable per-weapon firing state, in on-disk field order.
/// For now only `ammo` is used
/// at runtime (current rounds in the clip); the rest are parsed so the chunk
/// round-trips and is available later (condition, fire setting, modification
/// level, silencing).
///
/// The shipped chunk is 16 bytes (it omits the trailing silencing value), while
/// the 20-byte variant carries it. [`PropGunState::read`] therefore reads
/// each field only if `len` covers it, then snaps to the end of the chunk - so
/// both variants parse without over-reading. (A fixed 20-byte read panicked the
/// chunk-length assertion on every mission.)
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunState {
    /// Current rounds in the clip.
    pub ammo: i32,
    /// Weapon condition as a percentage (shipped guns author 100.0 = perfect).
    pub condition: f32,
    /// Current fire setting; indexes `PropBaseGunDesc::settings`.
    pub setting: i32,
    /// Current modification level.
    pub modification: i32,
    /// Amount of silencing, 0..1 (1 = perfect).
    pub silence_value: f32,
}

impl PropGunState {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropGunState {
        let start = reader.stream_position().unwrap();
        let ammo = read_i32(reader);
        // Condition is a 0..100 percentage; a record too short to carry one
        // describes an unworn gun.
        let condition = if len >= 8 { read_single(reader) } else { 100.0 };
        let setting = if len >= 12 { read_i32(reader) } else { 0 };
        let modification = if len >= 16 { read_i32(reader) } else { 0 };
        let silence_value = if len >= 20 { read_single(reader) } else { 0.0 };
        // Snap to the end of the chunk so both the 16- and 20-byte variants parse.
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
        PropGunState {
            ammo,
            condition,
            setting,
            modification,
            silence_value,
        }
    }
}
