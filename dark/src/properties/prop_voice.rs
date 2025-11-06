use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropVoiceIndex(pub i32);

impl PropVoiceIndex {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropVoiceIndex {
        let index = read_i32(reader);
        PropVoiceIndex(index)
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSpeechVoice(pub String);

impl PropSpeechVoice {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropSpeechVoice {
        let raw = read_string_with_size(reader, len as usize);
        let cleaned = raw.trim_end_matches('\0').to_ascii_lowercase();
        PropSpeechVoice(cleaned)
    }
}
