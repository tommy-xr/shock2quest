use std::io;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use shipyard::Component;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(FromPrimitive, Clone, Debug, Deserialize, Serialize)]
pub enum AnimTexFlags {
    WRAP = 0,
    RANDING = 1, // ?
    REVERSE = 2,
    PORTAL = 3, // ?
}

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropAnimTex {
    pub rate_in_milliseconds: u32,
    pub anim_flags: AnimTexFlags,
}

impl PropAnimTex {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropAnimTex {
        // TODO: Look at the rotate state in earth, see if it works?
        let rate_in_milliseconds = read_u32(reader);
        let anim_flags_bits = read_u32(reader);

        let anim_flags = AnimTexFlags::from_u32(anim_flags_bits).unwrap();
        PropAnimTex {
            rate_in_milliseconds,
            anim_flags,
        }
    }
}
