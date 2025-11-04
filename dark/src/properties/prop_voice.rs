use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::read_i32;

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropVoiceIndex(pub i32);

impl PropVoiceIndex {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropVoiceIndex {
        let index = read_i32(reader);
        PropVoiceIndex(index)
    }
}
