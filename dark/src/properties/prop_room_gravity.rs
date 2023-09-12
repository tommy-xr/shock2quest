use std::io;

use shipyard::Component;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Gravity {
    Reset,
    Set(f32),
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRoomGravity(pub Gravity);

impl PropRoomGravity {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropRoomGravity {
        let gravity_int = read_i32(reader);
        // if gravity_int == 0 {
        //     PropRoomGravity(Gravity::Reset)
        // } else {
        let mut gravity = (gravity_int as f32) / 100.0;

        // HACK: Gravity is a little wonky right now (it's not a force, a velocity),
        // so make some tweaks to speed up the upward gravity...
        if gravity < 0.0 {
            gravity *= 3.0;
        }

        PropRoomGravity(Gravity::Set(gravity))
        // }
    }
}
