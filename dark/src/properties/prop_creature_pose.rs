use std::io;

use shipyard::Component;

use crate::ss2_common::*;
use bitflags::bitflags;

use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct PoseType: u32 {
        const TAG = 0;
        const MOTION_NAME = 1;
        const INVALID = 2;
        //const Default = Self::Invalid;
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropCreaturePose {
    pub pose_type: PoseType,
    pub motion_or_tag_name: String,
    //pub unk: f32, // what is this?
    pub scale: f32,
    pub ballistic: bool,
}

impl PropCreaturePose {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropCreaturePose {
        let pose_type_bits = read_u32(reader);
        let motion_name = read_string_with_size(reader, 80);
        let _unk = read_single(reader);
        let scale = read_single(reader);
        let ballistic = read_bool(reader);

        PropCreaturePose {
            pose_type: PoseType::from_bits(pose_type_bits).unwrap_or(PoseType::INVALID),
            motion_or_tag_name: motion_name,
            scale,
            ballistic,
        }
    }
}
