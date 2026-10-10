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

/// Global mute flag in P$AI_BcstSe. Per-concept customization is not yet used.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIBroadcastDisabled(pub bool);

impl PropAIBroadcastDisabled {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        let disabled = read_i32(reader) != 0;
        let _ = crate::ss2_common::read_bytes(reader, len.saturating_sub(4) as usize);
        Self(disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broadcast_mute_consumes_the_customization_record() {
        for disabled in [0i32, 1] {
            let mut bytes = vec![0; 712];
            bytes[..4].copy_from_slice(&disabled.to_le_bytes());
            let mut reader = std::io::Cursor::new(bytes);
            assert_eq!(
                PropAIBroadcastDisabled::read(&mut reader, 712).0,
                disabled != 0
            );
            assert_eq!(reader.position(), 712);
        }
    }
}
