use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::*;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KeyCard {
    pub is_master: bool,
    pub region_id: u32,
    pub lock_id: u8,
}

impl KeyCard {
    pub fn can_unlock(&self, key_dst: &KeyCard) -> bool {
        let region_matches = self.region_id == key_dst.region_id;
        if self.is_master && region_matches {
            return true;
        }

        let lock_id_matches = self.lock_id == key_dst.lock_id;

        region_matches && lock_id_matches
    }
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> KeyCard {
        let is_master = read_bool_u8(reader);
        let region_id = read_u32(reader);
        let lock_id = read_u8(reader);
        KeyCard {
            is_master,
            region_id,
            lock_id,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropKeySrc(pub KeyCard);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropKeyDst(pub KeyCard);
