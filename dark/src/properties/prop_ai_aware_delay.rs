use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_i32};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIAwareDelay {
    pub to_two: i32,
    pub to_three: i32,
    pub two_reuse: i32,
    pub three_reuse: i32,
    pub ignore_range: i32,
}

impl PropAIAwareDelay {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAIAwareDelay {
        let to_two = read_i32(reader);
        let to_three = read_i32(reader);
        let two_reuse = read_i32(reader);
        let three_reuse = read_i32(reader);
        let ignore_range = read_i32(reader);

        const EXPECTED_SIZE: u32 = 20;
        if len > EXPECTED_SIZE {
            let remaining = (len - EXPECTED_SIZE) as usize;
            read_bytes(reader, remaining);
        }

        PropAIAwareDelay {
            to_two,
            to_three,
            two_reuse,
            three_reuse,
            ignore_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::PropAIAwareDelay;

    #[test]
    fn reads_the_retail_negative_reaction_time_sentinel() {
        let mut bytes = Vec::new();
        for value in [-1_i32, 4000, 10000, 10000, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let delay = PropAIAwareDelay::read(&mut Cursor::new(bytes), 20);

        assert_eq!(delay.to_two, -1);
        assert_eq!(delay.to_three, 4000);
    }
}
