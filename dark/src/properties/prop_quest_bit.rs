use std::io;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::read_u32;

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct QuestBitValue: u32 {
        const UNKNOWN = 0; // The user does not know about the quest yet
        const INCOMPLETE = 1; // The user knows about the quest, but has not completed iet yet
        const COMPLETE = 2; // The user has completed the quest
    }
}

#[derive(Component, Clone, Debug, Deserialize, Serialize)]
pub struct PropQuestBitName(pub String);

#[derive(Component, Clone, Debug, Deserialize, Serialize)]
pub struct PropQuestBitValue(pub QuestBitValue);

impl PropQuestBitValue {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropQuestBitValue {
        let v = read_u32(reader);
        let qb_val = QuestBitValue::from_bits(v).unwrap();
        PropQuestBitValue(qb_val)
    }
}
