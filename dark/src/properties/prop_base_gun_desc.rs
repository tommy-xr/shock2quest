use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single, read_u32};

use serde::{Deserialize, Serialize};

/// `P$BaseGunDe` - the per-archetype gun description. Mirrors the leading fields
/// of the Dark engine's `sBaseGunDesc` (engfeat/gunbase.h), in field order. The
/// SS2 chunk is larger than this 40-byte Thief layout (it carries per-setting
/// data too), so [`PropBaseGunDesc::read`] reads only the leading fields and
/// then seeks to the end of the chunk (`len`), leaving the rest available later.
///
/// For now only `clip` (magazine size) is used at runtime, to refill ammo on
/// reload; the rest are parsed so the values are available later (per-shot
/// ammo usage, reload time, fire timing).
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropBaseGunDesc {
    /// `m_burst` - projectiles per trigger pull (over time).
    pub burst: i32,
    /// `m_clip` - magazine size (rounds per full clip).
    pub clip: i32,
    /// `m_spray` - projectiles per trigger pull (instantaneous).
    pub spray: i32,
    /// `m_stimModifier` - multiplier on projectile stimulus intensity.
    pub stim_modifier: f32,
    /// `m_burstInterval` - interval between bursts (`tSimTime`, milliseconds).
    pub burst_interval_ms: u32,
    /// `m_shotInterval` - interval between shots (`tSimTime`, milliseconds).
    pub shot_interval_ms: u32,
    /// `m_ammoUsage` - ammo units consumed per shot.
    pub ammo_usage: i32,
    /// `m_speedModifier` - multiplier on projectile speed.
    pub speed_modifier: f32,
    /// `m_reloadTime` - time to reload (`tSimTime`, milliseconds).
    pub reload_time_ms: u32,
}

impl PropBaseGunDesc {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropBaseGunDesc {
        let start = reader.stream_position().unwrap();

        let burst = read_i32(reader);
        let clip = read_i32(reader);
        let spray = read_i32(reader);
        let stim_modifier = read_single(reader);
        let burst_interval_ms = read_u32(reader);
        let shot_interval_ms = read_u32(reader);
        let ammo_usage = read_i32(reader);
        let speed_modifier = read_single(reader);
        let reload_time_ms = read_u32(reader);

        // The SS2 chunk carries more than the leading fields above; snap to the
        // end of the property so the chunk round-trips regardless of its size.
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();

        PropBaseGunDesc {
            burst,
            clip,
            spray,
            stim_modifier,
            burst_interval_ms,
            shot_interval_ms,
            ammo_usage,
            speed_modifier,
            reload_time_ms,
        }
    }
}
