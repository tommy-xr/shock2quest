use std::io;

use crate::ss2_common::{read_i32, read_single, read_u32};

#[derive(Clone, Debug)]
pub struct MotionSchema {
    pub archetype_index: i32,
    pub schema_id: u32,
    pub flags: u32,
    pub time_modifier: f32,
    pub dist_modifier: f32,
    pub motion_index_list: Vec<u32>,
}

impl MotionSchema {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T) -> MotionSchema {
        let archetype_index = read_i32(reader);
        let schema_id = read_u32(reader);
        let flags = read_u32(reader);
        let time_modifier = read_single(reader);
        let dist_modifier = read_single(reader);

        let size = read_u32(reader);
        let mut motion_index_list = Vec::new();
        for _ in 0..size {
            motion_index_list.push(read_u32(reader));
        }

        MotionSchema {
            archetype_index,
            schema_id,
            flags,
            time_modifier,
            dist_modifier,
            motion_index_list,
        }
    }
}

const FLAG_FIXED_DURATION: u32 = 0x2;
const FLAG_TIME_WARP: u32 = 0x4;
const FLAG_STRETCH: u32 = 0x8;
const FLAG_FIXED_DISTANCE: u32 = 0x10;

/// A schema's authored playback modifiers. A fixed duration overrides a time
/// warp and a fixed distance overrides a stretch, as in the original.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SchemaTiming {
    flags: u32,
    time_modifier: f32,
    dist_modifier: f32,
}

impl SchemaTiming {
    /// Factor a clip of `clip_duration` seconds has its duration scaled by.
    pub fn time_scale(&self, clip_duration: f32) -> f32 {
        let scale = if self.flags & FLAG_FIXED_DURATION != 0 {
            if clip_duration > 0.0 {
                self.time_modifier / clip_duration
            } else {
                1.0
            }
        } else if self.flags & FLAG_TIME_WARP != 0 {
            self.time_modifier
        } else {
            1.0
        };
        // Non-positive or non-finite plays as authored; the clamp keeps
        // `Duration` arithmetic in range.
        if scale.is_finite() && scale > 0.0 {
            scale.clamp(0.01, 1000.0)
        } else {
            1.0
        }
    }

    /// Factor a clip's root travel is scaled by. `clip_distance` is the clip's
    /// translation length in port units; a fixed distance is authored in Dark
    /// feet, and a near-stationary clip is left unstretched.
    pub fn stretch(&self, clip_distance: f32) -> f32 {
        let stretch = if self.flags & FLAG_FIXED_DISTANCE != 0 {
            let clip_feet = clip_distance * crate::SCALE_FACTOR;
            if clip_feet > 0.1 {
                self.dist_modifier / clip_feet
            } else {
                1.0
            }
        } else if self.flags & FLAG_STRETCH != 0 {
            self.dist_modifier
        } else {
            1.0
        };
        // Zero is a valid authored stretch (travel nowhere); only a negative
        // or non-finite value plays as authored.
        if stretch.is_finite() && stretch >= 0.0 {
            stretch
        } else {
            1.0
        }
    }
}

impl MotionSchema {
    pub fn timing(&self) -> SchemaTiming {
        SchemaTiming {
            flags: self.flags,
            time_modifier: self.time_modifier,
            dist_modifier: self.dist_modifier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing(flags: u32, time_modifier: f32, dist_modifier: f32) -> SchemaTiming {
        SchemaTiming {
            flags,
            time_modifier,
            dist_modifier,
        }
    }

    #[test]
    fn unflagged_schema_plays_as_authored() {
        let timing = timing(0x1, 0.5, 2.0);
        assert_eq!(timing.time_scale(2.0), 1.0);
        assert_eq!(timing.stretch(1.0), 1.0);
    }

    #[test]
    fn time_warp_and_stretch_apply_their_modifiers() {
        // The Rumbler's run schema: 0xd = swizzle | time warp | stretch.
        let timing = timing(0xd, 0.7, 1.8);
        assert_eq!(timing.time_scale(2.0), 0.7);
        assert_eq!(timing.stretch(1.0), 1.8);
    }

    #[test]
    fn fixed_duration_and_distance_override_warp_and_stretch() {
        let timing = timing(
            FLAG_FIXED_DURATION | FLAG_TIME_WARP | FLAG_FIXED_DISTANCE | FLAG_STRETCH,
            3.0,
            4.0,
        );
        assert_eq!(timing.time_scale(2.0), 1.5);
        // 4 feet authored over a clip that travels 1 foot.
        assert_eq!(timing.stretch(1.0 / crate::SCALE_FACTOR), 4.0);
    }

    #[test]
    fn degenerate_inputs_play_as_authored() {
        assert_eq!(timing(FLAG_FIXED_DURATION, 3.0, 0.0).time_scale(0.0), 1.0);
        assert_eq!(timing(FLAG_FIXED_DISTANCE, 0.0, 4.0).stretch(0.0), 1.0);
        assert_eq!(timing(FLAG_TIME_WARP, f32::NAN, 0.0).time_scale(1.0), 1.0);
        assert_eq!(timing(FLAG_TIME_WARP, 1e-30, 0.0).time_scale(1.0), 0.01);
        assert_eq!(timing(FLAG_STRETCH, 0.0, -1.0).stretch(1.0), 1.0);
    }

    #[test]
    fn zero_and_long_distances_are_kept() {
        assert_eq!(timing(FLAG_FIXED_DISTANCE, 0.0, 0.0).stretch(1.0), 0.0);
        assert_eq!(
            timing(FLAG_FIXED_DISTANCE, 0.0, 30.0).stretch(1.0 / crate::SCALE_FACTOR),
            30.0
        );
    }
}
