use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single, read_u32};

use serde::{Deserialize, Serialize};

/// Fire settings a gun description carries. A gun's UI exposes the first two
/// (the two selectable fire modes); the third is an unselected leftover.
pub const GUN_SETTING_COUNT: usize = 3;

/// On-disk size of one [`GunSettingDesc`]: the nine fields below plus a
/// trailing i32 that the shipped data never initializes.
const SETTING_SIZE: u64 = 40;

/// One fire setting of a gun. Retail guns author different values per setting -
/// e.g. the pistol's second setting is a 3-round burst with a longer gap
/// between pulls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GunSettingDesc {
    /// Projectiles per trigger pull, over time. `-1` is unlimited (full auto).
    pub burst: i32,
    /// Magazine size (rounds per full clip).
    pub clip: i32,
    /// Projectiles per trigger pull, instantaneous.
    pub spray: i32,
    /// Multiplier on projectile stimulus intensity.
    pub stim_modifier: f32,
    /// Interval between bursts, in milliseconds.
    pub burst_interval_ms: u32,
    /// Interval between shots, in milliseconds.
    pub shot_interval_ms: u32,
    /// Ammo units consumed per shot.
    pub ammo_usage: i32,
    /// Multiplier on projectile speed.
    pub speed_modifier: f32,
    /// Time to reload, in milliseconds.
    pub reload_time_ms: u32,
}

impl Default for GunSettingDesc {
    fn default() -> Self {
        GunSettingDesc {
            burst: 1,
            clip: 0,
            spray: 1,
            stim_modifier: 1.0,
            burst_interval_ms: 0,
            shot_interval_ms: 0,
            ammo_usage: 1,
            speed_modifier: 1.0,
            reload_time_ms: 0,
        }
    }
}

impl GunSettingDesc {
    fn read<T: io::Read + io::Seek>(reader: &mut T) -> GunSettingDesc {
        let burst = read_i32(reader);
        let clip = read_i32(reader);
        let spray = read_i32(reader);
        let stim_modifier = read_single(reader);
        let burst_interval_ms = read_u32(reader);
        let shot_interval_ms = read_u32(reader);
        let ammo_usage = read_i32(reader);
        let speed_modifier = read_single(reader);
        let reload_time_ms = read_u32(reader);
        // Trailing i32 of the on-disk record; never initialized in shipped data.
        let _pad = read_i32(reader);

        GunSettingDesc {
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

/// `P$BaseGunDe` - a gun archetype's per-setting firing description: an array
/// of [`GUN_SETTING_COUNT`] fixed-size records, selected at runtime by
/// `PropGunState::setting`.
///
/// The shipped chunk is 136 bytes - the three 40-byte records plus a 16-byte
/// tail of uninitialized memory, which is never parsed.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropBaseGunDesc {
    pub settings: [GunSettingDesc; GUN_SETTING_COUNT],
}

impl PropBaseGunDesc {
    /// The description for fire setting `index`, falling back to setting 0 for
    /// an index the gun does not have.
    pub fn setting(&self, index: i32) -> &GunSettingDesc {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.settings.get(index))
            .unwrap_or(&self.settings[0])
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropBaseGunDesc {
        let start = reader.stream_position().unwrap();

        // Only records the chunk actually covers are read; a short chunk (never
        // seen in shipped data) defaults the rest rather than reading past it.
        let settings = std::array::from_fn(|index| {
            if (index as u64 + 1) * SETTING_SIZE <= len as u64 {
                GunSettingDesc::read(reader)
            } else {
                GunSettingDesc::default()
            }
        });

        // Snap past the uninitialized tail so the chunk round-trips.
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();

        PropBaseGunDesc { settings }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{GunSettingDesc, PropBaseGunDesc};

    fn setting_bytes(desc: &GunSettingDesc, pad: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(desc.burst.to_le_bytes());
        out.extend(desc.clip.to_le_bytes());
        out.extend(desc.spray.to_le_bytes());
        out.extend(desc.stim_modifier.to_le_bytes());
        out.extend(desc.burst_interval_ms.to_le_bytes());
        out.extend(desc.shot_interval_ms.to_le_bytes());
        out.extend(desc.ammo_usage.to_le_bytes());
        out.extend(desc.speed_modifier.to_le_bytes());
        out.extend(desc.reload_time_ms.to_le_bytes());
        out.extend(pad.to_le_bytes());
        out
    }

    /// The pistol as shipped: single shot, then a 3-round burst.
    fn pistol_settings() -> [GunSettingDesc; 3] {
        let normal = GunSettingDesc {
            burst: 1,
            clip: 12,
            spray: 1,
            stim_modifier: 1.0,
            burst_interval_ms: 0,
            shot_interval_ms: 500,
            ammo_usage: 1,
            speed_modifier: 1.0,
            reload_time_ms: 500,
        };
        let burst = GunSettingDesc {
            burst: 3,
            burst_interval_ms: 10,
            shot_interval_ms: 700,
            ..normal.clone()
        };
        [normal.clone(), burst, normal]
    }

    /// A retail-shaped chunk: three records plus a garbage tail.
    fn pistol_chunk() -> Vec<u8> {
        let mut bytes = Vec::new();
        for (index, setting) in pistol_settings().iter().enumerate() {
            bytes.extend(setting_bytes(setting, index as i32 * -7));
        }
        bytes.extend([0xAB; 16]);
        bytes
    }

    #[test]
    fn parses_every_setting_and_ignores_the_tail() {
        let chunk = pistol_chunk();
        let len = chunk.len() as u32;
        assert_eq!(len, 136);
        let mut cursor = Cursor::new(chunk);

        let desc = PropBaseGunDesc::read(&mut cursor, len);

        assert_eq!(desc.settings, pistol_settings());
        assert_eq!(cursor.position(), len as u64);
    }

    #[test]
    fn setting_selects_by_index_and_falls_back_to_the_first() {
        let desc = PropBaseGunDesc::read(&mut Cursor::new(pistol_chunk()), 136);

        assert_eq!(desc.setting(0).burst, 1);
        assert_eq!(desc.setting(1).burst, 3);
        assert_eq!(desc.setting(1).shot_interval_ms, 700);
        assert_eq!(desc.setting(3), desc.setting(0), "out of range");
        assert_eq!(desc.setting(-1), desc.setting(0), "negative");
    }

    #[test]
    fn a_short_chunk_defaults_the_records_it_does_not_cover() {
        let settings = pistol_settings();
        let chunk = setting_bytes(&settings[0], 0);
        let len = chunk.len() as u32;
        let mut cursor = Cursor::new(chunk);

        let desc = PropBaseGunDesc::read(&mut cursor, len);

        assert_eq!(desc.settings[0], settings[0]);
        assert_eq!(desc.settings[1], GunSettingDesc::default());
        assert_eq!(desc.settings[2], GunSettingDesc::default());
        assert_eq!(cursor.position(), len as u64, "never reads past the chunk");
    }
}
