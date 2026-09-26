use serde::{Deserialize, Serialize};
use shipyard::Component;
use std::io;

use crate::ss2_common::{read_i32, read_u16};

/// P$Projectil: packed sProjectile, distinct from the weapon's Projectile link
/// and BaseGunDesc.spray. One shell launches `count` independently aimed pellets.
#[derive(Debug, Clone, Copy, Component, Serialize, Deserialize, PartialEq, Eq)]
pub struct PropProjectile {
    pub count: i32,
    /// Unsigned Dark angle units (65536 units = one turn).
    pub spread: u16,
}

impl Default for PropProjectile {
    fn default() -> Self {
        Self {
            count: 1,
            spread: 0,
        }
    }
}

impl PropProjectile {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> Self {
        Self {
            count: read_i32(reader),
            spread: read_u16(reader),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_both_shipped_pellet_records_without_padding() {
        let mut bytes = io::Cursor::new([6, 0, 0, 0, 0, 4]);
        assert_eq!(
            PropProjectile::read(&mut bytes, 6),
            PropProjectile {
                count: 6,
                spread: 1024
            }
        );
        assert_eq!(bytes.position(), 6);
    }
}
