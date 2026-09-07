use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::{
    properties::GUN_SETTING_COUNT,
    ss2_common::{read_single, read_u16},
};

use serde::{Deserialize, Serialize};

/// Angles are stored as 16-bit turns: `0x10000` units make a full circle.
const DEGREES_PER_ANGLE_UNIT: f32 = 360.0 / 65536.0;

/// On-disk size of one [`GunKickSetting`]: the eleven fields below, with the
/// six 16-bit angles packed in pairs between the 32-bit floats.
const SETTING_SIZE: u64 = 32;

/// One fire setting's authored recoil. Angles are converted to degrees at
/// parse time; the remaining fields are the raw authored scalars.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GunKickSetting {
    /// Fraction of the kick applied before the shot leaves the barrel.
    pub pre_kick_pct: f32,
    /// Pitch added to the gun per shot, in degrees.
    pub kick_pitch_degrees: f32,
    /// Ceiling on accumulated kick pitch, in degrees.
    pub kick_pitch_max_degrees: f32,
    /// Heading added to the gun per shot, in degrees.
    pub kick_heading_degrees: f32,
    /// Rate the accumulated kick angles return to rest, in degrees.
    pub kick_angular_return_rate_degrees: f32,
    /// Displacement of the gun per shot (negative drives it towards the
    /// player).
    pub kick_back: f32,
    /// Ceiling on accumulated kick back.
    pub kick_back_max: f32,
    /// Rate the accumulated kick back returns to rest.
    pub kick_back_return_rate: f32,
    /// Pitch imparted to the player, in degrees.
    pub jolt_pitch_degrees: f32,
    /// Heading imparted to the player, in degrees.
    pub jolt_heading_degrees: f32,
    /// Backwards shove imparted to the player.
    pub jolt_back: f32,
}

impl GunKickSetting {
    fn read<T: io::Read>(reader: &mut T) -> GunKickSetting {
        let pre_kick_pct = read_single(reader);
        let kick_pitch_degrees = read_angle(reader);
        let kick_pitch_max_degrees = read_angle(reader);
        let kick_heading_degrees = read_angle(reader);
        let kick_angular_return_rate_degrees = read_angle(reader);
        let kick_back = read_single(reader);
        let kick_back_max = read_single(reader);
        let kick_back_return_rate = read_single(reader);
        let jolt_pitch_degrees = read_angle(reader);
        let jolt_heading_degrees = read_angle(reader);
        let jolt_back = read_single(reader);

        GunKickSetting {
            pre_kick_pct,
            kick_pitch_degrees,
            kick_pitch_max_degrees,
            kick_heading_degrees,
            kick_angular_return_rate_degrees,
            kick_back,
            kick_back_max,
            kick_back_return_rate,
            jolt_pitch_degrees,
            jolt_heading_degrees,
            jolt_back,
        }
    }
}

fn read_angle<T: io::Read>(reader: &mut T) -> f32 {
    read_u16(reader) as f32 * DEGREES_PER_ANGLE_UNIT
}

/// `P$GunKick` - a gun archetype's authored recoil, one [`GunKickSetting`] per
/// fire setting, selected by the same index as [`super::PropBaseGunDesc`].
///
/// Every shipped record is exactly `GUN_SETTING_COUNT * SETTING_SIZE` bytes.
/// Only the two selectable settings are authored; the third is zeroed.
#[derive(Debug, Component, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropGunKick {
    pub settings: [GunKickSetting; GUN_SETTING_COUNT],
}

impl PropGunKick {
    /// The kick for fire setting `index`, falling back to setting 0 for an
    /// index the gun does not have.
    pub fn setting(&self, index: i32) -> &GunKickSetting {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.settings.get(index))
            .unwrap_or(&self.settings[0])
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropGunKick {
        let start = reader.stream_position().unwrap();

        // A short record (never seen in shipped data) defaults the settings it
        // does not cover rather than reading past the chunk.
        let settings = std::array::from_fn(|index| {
            if (index as u64 + 1) * SETTING_SIZE <= len as u64 {
                GunKickSetting::read(reader)
            } else {
                GunKickSetting::default()
            }
        });

        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();

        PropGunKick { settings }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{GunKickSetting, PropGunKick, SETTING_SIZE};
    use crate::properties::GUN_SETTING_COUNT;

    fn setting_bytes(
        pre_kick_pct: f32,
        kick_angles: [u16; 4],
        backs: [f32; 3],
        jolt_angles: [u16; 2],
        jolt_back: f32,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(pre_kick_pct.to_le_bytes());
        for angle in kick_angles {
            out.extend(angle.to_le_bytes());
        }
        for back in backs {
            out.extend(back.to_le_bytes());
        }
        for angle in jolt_angles {
            out.extend(angle.to_le_bytes());
        }
        out.extend(jolt_back.to_le_bytes());
        out
    }

    /// The pistol as shipped: single shot, then a 3-round burst that drives the
    /// gun back further. The third setting is zeroed.
    fn pistol_chunk() -> Vec<u8> {
        let mut bytes = setting_bytes(
            0.0,
            [1280, 1280, 0, 2048],
            [-0.35, -0.35, 1.0],
            [1200, 0],
            1.0,
        );
        bytes.extend(setting_bytes(
            0.0,
            [1280, 1280, 0, 2048],
            [-0.3, -0.6, 1.0],
            [1200, 0],
            1.0,
        ));
        bytes.extend(setting_bytes(0.0, [0; 4], [0.0; 3], [0; 2], 0.0));
        bytes
    }

    #[test]
    fn the_record_is_three_fixed_size_settings() {
        assert_eq!(
            pistol_chunk().len() as u64,
            SETTING_SIZE * GUN_SETTING_COUNT as u64
        );
    }

    /// Pins the shipped pistol values, in degrees.
    #[test]
    fn parses_every_setting_of_the_shipped_pistol_record() {
        let chunk = pistol_chunk();
        let len = chunk.len() as u32;
        let mut cursor = Cursor::new(chunk);

        let kick = PropGunKick::read(&mut cursor, len);

        assert_eq!(cursor.position(), len as u64);
        assert_eq!(kick.settings[0].pre_kick_pct, 0.0);
        assert_eq!(kick.settings[0].kick_pitch_degrees, 7.03125);
        assert_eq!(kick.settings[0].kick_pitch_max_degrees, 7.03125);
        assert_eq!(kick.settings[0].kick_heading_degrees, 0.0);
        assert_eq!(kick.settings[0].kick_angular_return_rate_degrees, 11.25);
        assert_eq!(kick.settings[0].kick_back, -0.35);
        assert_eq!(kick.settings[0].kick_back_max, -0.35);
        assert_eq!(kick.settings[0].kick_back_return_rate, 1.0);
        assert_eq!(kick.settings[0].jolt_pitch_degrees, 6.591_796_9);
        assert_eq!(kick.settings[0].jolt_heading_degrees, 0.0);
        assert_eq!(kick.settings[0].jolt_back, 1.0);

        // The burst setting only differs in how far back it drives the gun.
        assert_eq!(kick.settings[1].kick_pitch_degrees, 7.03125);
        assert_eq!(kick.settings[1].kick_back, -0.3);
        assert_eq!(kick.settings[1].kick_back_max, -0.6);

        assert_eq!(kick.settings[2], GunKickSetting::default());
    }

    #[test]
    fn setting_selects_by_index_and_falls_back_to_the_first() {
        let kick = PropGunKick::read(&mut Cursor::new(pistol_chunk()), 96);

        assert_eq!(kick.setting(1).kick_back, -0.3);
        assert_eq!(kick.setting(3), kick.setting(0), "out of range");
        assert_eq!(kick.setting(-1), kick.setting(0), "negative");
    }

    #[test]
    fn a_short_record_defaults_the_settings_it_does_not_cover() {
        let chunk = pistol_chunk()[..SETTING_SIZE as usize].to_vec();
        let len = chunk.len() as u32;
        let mut cursor = Cursor::new(chunk);

        let kick = PropGunKick::read(&mut cursor, len);

        assert_eq!(kick.settings[0].kick_back, -0.35);
        assert_eq!(kick.settings[1], GunKickSetting::default());
        assert_eq!(kick.settings[2], GunKickSetting::default());
        assert_eq!(cursor.position(), len as u64, "never reads past the record");
    }
}
