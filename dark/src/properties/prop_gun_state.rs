use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single};

use serde::{Deserialize, Serialize};

/// `P$GunState` - the mutable per-weapon firing state. Mirrors the Dark engine's
/// `sGunState` (engfeat/gunbase.h), in field order. For now only `ammo` is used
/// at runtime (current rounds in the clip); the rest are parsed so the chunk
/// round-trips and is available later (condition, fire setting, modification
/// level, silencing).
///
/// SS2's on-disk chunk is 16 bytes (it omits the trailing `m_silenceValue`),
/// while Thief's `sGunState` is 20 bytes. [`PropGunState::read`] therefore reads
/// each field only if `len` covers it, then snaps to the end of the chunk - so
/// both variants parse without over-reading. (A fixed 20-byte read panicked the
/// chunk-length assertion on every mission.)
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunState {
    /// `m_ammoCount` - current rounds in the clip.
    pub ammo: i32,
    /// `m_condition` - 0..1 weapon condition (1 = perfect).
    pub condition: f32,
    /// `m_setting` - current fire setting.
    pub setting: i32,
    /// `m_modification` - current modification level.
    pub modification: i32,
    /// `m_silenceValue` - 0..1 amount of silencing (1 = perfect).
    pub silence_value: f32,
}

impl PropGunState {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropGunState {
        let start = reader.stream_position().unwrap();
        let ammo = read_i32(reader);
        let condition = if len >= 8 { read_single(reader) } else { 1.0 };
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
