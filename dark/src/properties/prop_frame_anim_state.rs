use shipyard::Component;
use std::io;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropFrameAnimState {
    pub current_frame: u32,
}

impl PropFrameAnimState {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropFrameAnimState {
        let _unk1 = read_u32(reader);
        let _unk2 = read_u32(reader);
        let current_frame = read_u32(reader);
        let _unk3 = read_u32(reader);
        PropFrameAnimState { current_frame }
    }
}
