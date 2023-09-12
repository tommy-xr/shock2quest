use shipyard::Component;
use std::io;

use crate::ss2_common::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropBitmapAnimation {
    pub kill_on_completion: bool,
}

impl PropBitmapAnimation {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropBitmapAnimation {
        let kill_on_completion = read_bool(reader);
        PropBitmapAnimation { kill_on_completion }
    }
}
