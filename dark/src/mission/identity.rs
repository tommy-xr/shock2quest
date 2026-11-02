//! Authored campaign fingerprints, separate from mutable gameplay quest state.

use std::{collections::BTreeMap, io};

use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct MissionIdentity {
    pub mission: String,
    pub gamesys: Option<String>,
    pub quest_variables: BTreeMap<String, i32>,
}

impl MissionIdentity {
    pub fn read<T: io::Read + io::Seek>(
        mission: &str,
        toc: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> io::Result<Self> {
        let mut identity = Self {
            mission: mission.to_ascii_lowercase(),
            ..Self::default()
        };
        for name in ["GAM_FILE", "QUEST_DB"] {
            if !toc.has_chunk(name.to_owned()) {
                continue;
            }
            let bytes = super::read_chunk_bytes(toc, reader, name)
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, name))?;
            if name == "GAM_FILE" {
                identity.gamesys = Some(super::fixed_string(&bytes).to_ascii_lowercase());
            } else {
                identity.quest_variables = read_quests(&bytes)?;
            }
        }
        Ok(identity)
    }

    /// SCP object IDs are meaningful only in the matching mission and gamesys.
    pub fn matches_scp_patch(&self, mission: &str, loaded_gamesys: &str) -> bool {
        !self.mission.is_empty()
            && self.mission.eq_ignore_ascii_case(mission)
            && loaded_gamesys.eq_ignore_ascii_case("shockscp.gam")
            && self.gamesys.as_deref() == Some("shockscp.gam")
            && self.quest_variables.get("scplevel") == Some(&1)
    }
}

fn read_quests(mut bytes: &[u8]) -> io::Result<BTreeMap<String, i32>> {
    let mut quests = BTreeMap::new();
    while !bytes.is_empty() {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid QUEST_DB record");
        let length = bytes.get(..4).ok_or_else(invalid)?;
        let length = u32::from_le_bytes(length.try_into().unwrap()) as usize;
        bytes = &bytes[4..];
        // Length includes the terminator; the value immediately follows it.
        let name = bytes.get(..length).ok_or_else(invalid)?;
        let name = name.strip_suffix(&[0]).ok_or_else(invalid)?;
        if name.is_empty() || name.contains(&0) {
            return Err(invalid());
        }
        let name = std::str::from_utf8(name)
            .map_err(|_| invalid())?
            .to_ascii_lowercase();
        bytes = &bytes[length..];
        let value = bytes.get(..4).ok_or_else(invalid)?;
        let value = i32::from_le_bytes(value.try_into().unwrap());
        quests.insert(name, value);
        bytes = &bytes[4..];
    }
    Ok(quests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(chunks: &[(&str, &[u8])]) -> io::Result<MissionIdentity> {
        let mut bytes = vec![0u8; 272];
        let mut entries = Vec::new();
        for (name, data) in chunks {
            let offset = bytes.len() as u32;
            bytes.resize(bytes.len() + 24, 0);
            bytes.extend_from_slice(data);
            entries.push((*name, offset, data.len() as u32));
        }
        let offset = bytes.len() as u32;
        bytes[..4].copy_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (name, offset, length) in entries {
            let mut label = [0; 12];
            label[..name.len()].copy_from_slice(name.as_bytes());
            bytes.extend_from_slice(&label);
            bytes.extend_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&length.to_le_bytes());
        }
        let mut reader = Cursor::new(bytes);
        let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        MissionIdentity::read("MedSci1.MIS", &toc, &mut reader)
    }

    const QUESTS: &[u8] = b"\x0b\0\0\0patchlevel\0\x01\0\0\0\x09\0\0\0SCPLevel\0\x01\0\0\0";

    #[test]
    fn mission_identity_reads_authored_fingerprints_and_rejects_mixed_contexts() {
        let identity = read(&[("GAM_FILE", b"ShockSCP.GAM\0"), ("QUEST_DB", QUESTS)]).unwrap();
        assert_eq!(identity.gamesys.as_deref(), Some("shockscp.gam"));
        assert_eq!(identity.quest_variables.get("patchlevel"), Some(&1));
        assert!(identity.matches_scp_patch("MEDSCI1.MIS", "ShockSCP.GAM"));
        assert!(!identity.matches_scp_patch("ops1.mis", "shockscp.gam"));
        assert!(!identity.matches_scp_patch("medsci1.mis", "shock2.gam"));
        let mut unnamed = identity.clone();
        unnamed.mission.clear();
        assert!(!unnamed.matches_scp_patch("", "shockscp.gam"));
        for chunks in [
            vec![],
            vec![("GAM_FILE", &b"shock2.gam\0"[..]), ("QUEST_DB", QUESTS)],
            vec![("GAM_FILE", &b"shockscp.gam\0"[..])],
        ] {
            assert!(
                !read(&chunks)
                    .unwrap()
                    .matches_scp_patch("medsci1.mis", "shockscp.gam")
            );
        }
    }

    #[test]
    fn mission_identity_rejects_truncated_quests_without_accepting_partial_guards() {
        for end in 1..QUESTS.len() {
            // A complete first record is a valid QUEST_DB with no SCP guard.
            if end != 19 {
                assert!(
                    read(&[("QUEST_DB", &QUESTS[..end])]).is_err(),
                    "accepted {end} bytes"
                );
            }
        }
    }
}
