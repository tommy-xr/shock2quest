use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::read_i32;

/// Dark's `sWeaponSkills`: Standard, Energy, Heavy, Exotic. On an object,
/// `BaseWeaponDesc` is the skill requirement checked by CheckRequirements.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropBaseWeaponDesc(pub [i32; 4]);

impl PropBaseWeaponDesc {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 16, "a weapon-skill record contains four i32 values");
        Self(std::array::from_fn(|_| read_i32(reader)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn weapon_requirements_preserve_all_four_classes() {
        let mut bytes = Vec::new();
        for value in [6_i32, 2, 3, 4] {
            bytes.extend(value.to_le_bytes());
        }
        let mut reader = Cursor::new(bytes);
        assert_eq!(PropBaseWeaponDesc::read(&mut reader, 16).0, [6, 2, 3, 4]);
        assert_eq!(reader.position(), 16);
    }
}
