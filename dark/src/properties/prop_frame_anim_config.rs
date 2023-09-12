use shipyard::Component;
use std::io;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropFrameAnimConfig {
    pub frames_per_second: f32,
    pub clamp: bool,
    pub bounce: bool,
    pub frame_limit: bool,
    pub unk: u8, // What is this for?
}

impl PropFrameAnimConfig {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropFrameAnimConfig {
        let frames_per_second = read_single(reader);
        let clamp = read_bool_u8(reader);
        let bounce = read_bool_u8(reader);
        let frame_limit = read_bool_u8(reader);
        let unk = read_u8(reader);
        PropFrameAnimConfig {
            frames_per_second,
            clamp,
            bounce,
            frame_limit,
            unk,
        }
    }
}
