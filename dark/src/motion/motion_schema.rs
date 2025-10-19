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
