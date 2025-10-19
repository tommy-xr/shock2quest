// ss2_bin_header.rs
// Common header for ai/obj bin files
use std::io::prelude::*;

use crate::ss2_common::{self};

pub enum BinFileType {
    Mesh, // Animated AI Mesh
    Obj,  // Static object
}

pub struct SystemShock2BinHeader {
    pub bin_type: BinFileType,
    pub version: u32,
}

pub fn read<T: Read + Seek>(reader: &mut T) -> SystemShock2BinHeader {
    let header = ss2_common::read_string_with_size(reader, 4);

    let bin_type = match header.as_str() {
        "LGMD" => BinFileType::Obj,
        "LGMM" => BinFileType::Mesh,
        _ => panic!("Unexpected bin_type {header}"),
    };

    let version = ss2_common::read_u32(reader);
    SystemShock2BinHeader { bin_type, version }
}
