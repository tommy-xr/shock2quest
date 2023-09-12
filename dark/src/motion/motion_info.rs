use std::io;

use crate::ss2_common::{read_i32, read_single, read_string_with_size, read_u32};

#[derive(Debug)]
pub struct MotionInfo {
    pub motion_type: u32,
    pub sig: u32, // bitfield for the joints that this animation impacts
    pub frame_count: f32,
    pub frame_rate: i32,
    pub mot_num: i32, // maybe index into motion db?
    pub name: String,
}

impl MotionInfo {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> MotionInfo {
        let motion_type = read_u32(reader);
        let sig = read_u32(reader);
        let frame_count = read_single(reader);
        let frame_rate = read_i32(reader);
        let mot_num = read_i32(reader);
        let name = read_string_with_size(reader, 12);
        MotionInfo {
            motion_type,
            sig,
            frame_count,
            frame_rate,
            mot_num,
            name,
        }
    }
}
