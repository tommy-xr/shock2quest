use std::io;

use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size};
use serde::{Deserialize, Serialize};

const NUM_REPLICATOR_ITEMS: usize = 6;

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropReplicatorContents {
    pub costs: [i32; NUM_REPLICATOR_ITEMS],
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
            read_i32(reader),
            read_i32(reader),
            read_i32(reader),
            read_i32(reader),
            read_i32(reader),
            read_i32(reader),
        ];

        PropReplicatorContents {
            costs,
            object_names,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn contents_preserve_signed_authored_costs() {
        let mut bytes = vec![0; NUM_REPLICATOR_ITEMS * 64];
        for cost in [-1_i32, 0, 3, 4, 40, i32::MAX] {
            bytes.write_all(&cost.to_le_bytes()).unwrap();
        }

        let parsed = PropReplicatorContents::read(&mut Cursor::new(bytes), (6 * 64 + 6 * 4) as u32);
        assert_eq!(parsed.costs, [-1, 0, 3, 4, 40, i32::MAX]);
    }
}
