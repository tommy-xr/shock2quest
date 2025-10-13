use std::io;



use shipyard::Component;

use crate::ss2_common::{read_string_with_size, read_u32};
use serde::{Deserialize, Serialize};

const NUM_REPLICATOR_ITEMS: usize = 6;

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropReplicatorContents {
    pub costs: [u32; NUM_REPLICATOR_ITEMS],
    pub object_names: [String; NUM_REPLICATOR_ITEMS],
}

impl PropReplicatorContents {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropReplicatorContents {
        let object_names = [
            read_string_with_size(reader, 64).to_ascii_lowercase(),
            read_string_with_size(reader, 64).to_ascii_lowercase(),
            read_string_with_size(reader, 64).to_ascii_lowercase(),
            read_string_with_size(reader, 64).to_ascii_lowercase(),
            read_string_with_size(reader, 64).to_ascii_lowercase(),
            read_string_with_size(reader, 64).to_ascii_lowercase(),
        ];

        let costs = [
            read_u32(reader),
            read_u32(reader),
            read_u32(reader),
            read_u32(reader),
            read_u32(reader),
            read_u32(reader),
        ];

        PropReplicatorContents {
            costs,
            object_names,
        }
    }
}
