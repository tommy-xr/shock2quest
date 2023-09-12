use std::io;

use cgmath::Vector3;
use shipyard::Component;

use crate::ss2_common::{read_string_with_size, read_u16, read_u32, read_vec3};

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPlayerGun {
    pub flags: u32,
    pub hand_model: String,
    pub icon_file: String,
    pub model_offset: Vector3<f32>,
    pub fire_offset: Vector3<f32>,
    pub heading: u16,
    pub reload_pitch: u16,
    pub reload_rate: u16,
    pub gun_type: u32,
}

impl PropPlayerGun {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPlayerGun {
        let flags = read_u32(reader);
        let hand_model = read_string_with_size(reader, 16);
        let icon_file = read_string_with_size(reader, 16);
        let model_offset = read_vec3(reader);
        let fire_offset = read_vec3(reader);
        let heading = read_u16(reader);
        let reload_pitch = read_u16(reader);
        let reload_rate = read_u16(reader);
        let gun_type = read_u32(reader);

        PropPlayerGun {
            flags,
            hand_model,
            icon_file,
            model_offset,
            fire_offset,
            heading,
            reload_pitch,
            reload_rate,
            gun_type,
        }
    }
}
