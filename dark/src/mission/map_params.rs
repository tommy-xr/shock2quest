//! Mission-authored automap parameters (`MAPPARAM`).
//!
//! Looking Glass defines the version-1 file variable as `sMapParams`, a single
//! 32-bit `BOOL m_rotatehack` (`shock/shkparam.h` + `shock/shkparam.cpp`). The
//! file-var reset path zeroes its storage before a mission load, so a missing
//! or unreadable chunk faithfully defaults to the normal, non-hack mapping.

use std::io::{self, SeekFrom};

use tracing::warn;

use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;

/// Automap coordinate mapping authored for one mission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapParams {
    /// The original engine's exceptional 90-degree map-page mapping.
    pub rotate_hack: bool,
}

impl MapParams {
    /// Read the exact retail `MAPPARAM` v1.0 layout.
    ///
    /// Returns `None` for absent, malformed, or unknown-version chunks. The
    /// mission loader then uses [`MapParams::default`], matching Dark's
    /// zero-filled file-variable reset instead of guessing from map markers.
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<Self> {
        let chunk = table_of_contents.get_chunk("MAPPARAM".to_owned())?;
        if (chunk.version_major, chunk.version_minor) != (1, 0) || chunk.length != 4 {
            warn!(
                "Unsupported MAPPARAM chunk version {}.{} or length {}; using the default map mapping",
                chunk.version_major, chunk.version_minor, chunk.length
            );
            return None;
        }

        reader.seek(SeekFrom::Start(chunk.offset)).ok()?;
        let mut bytes = [0u8; 4];
        if reader.read_exact(&mut bytes).is_err() {
            warn!("Truncated MAPPARAM chunk; using the default map mapping");
            return None;
        }
        Some(Self {
            rotate_hack: u32::from_le_bytes(bytes) != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::ss2_chunk_file_reader;

    use super::MapParams;

    const CHUNK_OFFSET: u32 = 272;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_name(bytes: &mut Vec<u8>, name: &str) {
        let mut field = [0u8; 12];
        field[..name.len()].copy_from_slice(name.as_bytes());
        bytes.extend_from_slice(&field);
    }

    /// Minimal Dark tag file carrying one optional mission file-var chunk.
    fn chunk_file(payload: Option<&[u8]>, version: (u32, u32)) -> Cursor<Vec<u8>> {
        let payload_len = payload.map_or(0, <[u8]>::len) as u32;
        let inventory_offset = CHUNK_OFFSET + payload.map_or(0, |_| 24 + payload_len);
        let mut bytes = Vec::new();

        push_u32(&mut bytes, inventory_offset);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0; 256]);
        push_u32(&mut bytes, 0xEFBEADDE);
        assert_eq!(bytes.len(), CHUNK_OFFSET as usize);

        if let Some(payload) = payload {
            push_name(&mut bytes, "MAPPARAM");
            push_u32(&mut bytes, version.0);
            push_u32(&mut bytes, version.1);
            push_u32(&mut bytes, 0);
            bytes.extend_from_slice(payload);
            push_u32(&mut bytes, 1);
            push_name(&mut bytes, "MAPPARAM");
            push_u32(&mut bytes, CHUNK_OFFSET);
            push_u32(&mut bytes, payload_len);
        } else {
            push_u32(&mut bytes, 0);
        }

        Cursor::new(bytes)
    }

    fn read(payload: Option<&[u8]>, version: (u32, u32)) -> Option<MapParams> {
        let mut reader = chunk_file(payload, version);
        let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        MapParams::read(&table_of_contents, &mut reader)
    }

    #[test]
    fn reads_retail_v1_bool_layout() {
        assert_eq!(
            read(Some(&0u32.to_le_bytes()), (1, 0)),
            Some(MapParams { rotate_hack: false })
        );
        assert_eq!(
            read(Some(&1u32.to_le_bytes()), (1, 0)),
            Some(MapParams { rotate_hack: true })
        );
        // Dark's BOOL semantics treat every non-zero 32-bit value as true.
        assert_eq!(
            read(Some(&2u32.to_le_bytes()), (1, 0)),
            Some(MapParams { rotate_hack: true })
        );
    }

    #[test]
    fn rejects_absent_malformed_or_unknown_chunks() {
        assert_eq!(read(None, (1, 0)), None);
        assert_eq!(read(Some(&[1, 0, 0]), (1, 0)), None);
        assert_eq!(read(Some(&[1, 0, 0, 0, 0]), (1, 0)), None);
        assert_eq!(read(Some(&1u32.to_le_bytes()), (0, 9)), None);
        assert_eq!(read(Some(&1u32.to_le_bytes()), (1, 1)), None);
    }
}
