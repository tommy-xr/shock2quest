use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bool, read_bytes, read_i32, read_single};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIDevice {
    pub joint_activate: i32,
    pub inactive_pos: f32,
    pub active_pos: f32,
    pub activate_speed: f32,
    pub joint_rotate: i32,
    pub facing_epsilon: f32,
    pub activate_rotate: bool,
}

impl PropAIDevice {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIDevice {
        let joint_activate = read_i32(reader);
        let inactive_pos = read_single(reader);
        let active_pos = read_single(reader);
        let activate_speed = read_single(reader);
        let joint_rotate = read_i32(reader);
        let facing_epsilon = read_single(reader);

        const BASE_SIZE: u32 = 24;
        let extra = len.saturating_sub(BASE_SIZE);

        let (activate_rotate, remainder) = if extra >= 4 {
            (read_bool(reader), extra - 4)
        } else {
            (false, extra)
        };

        if remainder > 0 {
            read_bytes(reader, remainder as usize);
        }

        PropAIDevice {
            joint_activate,
            inactive_pos,
            active_pos,
            activate_speed,
            joint_rotate,
            facing_epsilon,
            activate_rotate,
        }
    }
}
