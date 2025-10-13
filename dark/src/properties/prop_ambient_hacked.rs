use std::io;

use bitflags::bitflags;
use num_traits::ToPrimitive;
use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size, read_u32};
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct AmbientSoundFlags: u32 {
        const ENVIRONMENTAL = 1 << 0; //1
        const NO_SHARP_CURVE = 1 << 1; //2
        const TURNED_OFF = 1 << 2; // 4
        const REMOVE_PROP = 1 << 3; //8
        const MUSIC = 1 << 4; //16
        const SYNCH = 1 << 5; //32
        const NO_FADE = 1 << 6;
        const DESTROY_OBJECT = 1 << 7;
        const DO_AUTO_OFF  = 1 << 8;
        const DEFAULT = 0;
    }
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropAmbientHacked {
    pub sound_flags: AmbientSoundFlags,
    pub radius: i32,
    pub radius_squared: f32,
    pub volume: i32,
    pub schema: String,
    pub aux1: String,
    pub aux2: String,
}

impl PropAmbientHacked {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropAmbientHacked {
        let radius = read_i32(reader);
        let radius_f32: f32 = radius.to_f32().unwrap();
        let radius_squared: f32 = radius_f32 * radius_f32;
        let volume = read_i32(reader);
        let sound_flag_bits = read_u32(reader);
        let sound_flags = AmbientSoundFlags::from_bits(sound_flag_bits).unwrap();
        let schema = read_string_with_size(reader, 16);
        let aux1 = read_string_with_size(reader, 16);
        let aux2 = read_string_with_size(reader, 16);

        PropAmbientHacked {
            radius,
            radius_squared,
            volume,
            sound_flags,
            schema,
            aux1,
            aux2,
        }
    }
}
