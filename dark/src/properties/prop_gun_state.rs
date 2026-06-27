use std::io;

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single};

use serde::{Deserialize, Serialize};

/// `P$GunState` - the mutable per-weapon firing state. Mirrors the Dark engine's
/// `sGunState` (engfeat/gunbase.h): 20 bytes, in field order. For now only
/// `ammo` is used at runtime (current rounds in the clip); the rest are parsed
/// so the chunk round-trips and is available later (condition, fire setting,
/// modification level, silencing).
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
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropGunState {
        let ammo = read_i32(reader);
        let condition = read_single(reader);
        let setting = read_i32(reader);
        let modification = read_i32(reader);
        let silence_value = read_single(reader);
        PropGunState {
            ammo,
            condition,
            setting,
            modification,
            silence_value,
        }
    }
}
