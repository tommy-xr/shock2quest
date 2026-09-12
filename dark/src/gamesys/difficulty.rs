//! Retail difficulty indices and the field-major, six-column DIFFPARAM file-var.
use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Seek},
    str::FromStr,
};

/// Only the four campaign choices are selectable. Retail reserves 0 for
/// Playtest and 5 for Multiplayer; neither is a single-player difficulty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
    Impossible,
}
impl Difficulty {
    pub const ALL: [Self; 4] = [Self::Easy, Self::Normal, Self::Hard, Self::Impossible];
    pub const fn retail_index(self) -> usize {
        match self {
            Self::Easy => 1,
            Self::Normal => 2,
            Self::Hard => 3,
            Self::Impossible => 4,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Normal => "Normal",
            Self::Hard => "Hard",
            Self::Impossible => "Impossible",
        }
    }
}
impl FromStr for Difficulty {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|d| d.label().eq_ignore_ascii_case(value))
            .ok_or("expected easy, normal, hard, or impossible")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DifficultyParams {
    pub trainer_multiplier: [f32; 6],
    pub base_hp: [i32; 6],
    pub hp_per_endurance: [i32; 6],
    pub base_psi: [i32; 6],
    pub psi_per_psionics: [i32; 6],
    pub loot_discard_threshold: [i32; 6],
    pub replicator_multiplier: [f32; 6],
}
impl DifficultyParams {
    pub fn read<T: Read + Seek>(toc: &ChunkFileTableOfContents, reader: &mut T) -> Option<Self> {
        let chunk = toc.get_chunk("DIFFPARAM".to_owned())?;
        if chunk.length < 168 {
            return None;
        }
        reader.seek(io::SeekFrom::Start(chunk.offset)).ok()?;
        Self::read_record(reader)
    }
    fn read_record(reader: &mut impl Read) -> Option<Self> {
        let mut value = Self {
            trainer_multiplier: [0.0; 6],
            base_hp: [0; 6],
            hp_per_endurance: [0; 6],
            base_psi: [0; 6],
            psi_per_psionics: [0; 6],
            loot_discard_threshold: [0; 6],
            replicator_multiplier: [0.0; 6],
        };
        reader
            .read_f32_into::<LittleEndian>(&mut value.trainer_multiplier)
            .ok()?;
        for row in [
            &mut value.base_hp,
            &mut value.hp_per_endurance,
            &mut value.base_psi,
            &mut value.psi_per_psionics,
            &mut value.loot_discard_threshold,
        ] {
            reader.read_i32_into::<LittleEndian>(row).ok()?;
        }
        reader
            .read_f32_into::<LittleEndian>(&mut value.replicator_multiplier)
            .ok()?;
        if value
            .trainer_multiplier
            .iter()
            .chain(&value.replicator_multiplier)
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return None;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn retail_record() -> Vec<u8> {
        let mut bytes = Vec::new();
        for v in [1.0_f32, 0.85, 1.0, 1.4, 1.8, 1.4] {
            bytes.extend(v.to_le_bytes());
        }
        for row in [
            [30_i32, 45, 30, 24, 7, 24],
            [5, 10, 5, 3, 3, 3],
            [5, 10, 5, 3, 1, 5],
            [10, 16, 10, 8, 5, 10],
            [0, 0, 0, 30, 75, 0],
        ] {
            for v in row {
                bytes.extend(v.to_le_bytes());
            }
        }
        for v in [1.0_f32, 0.85, 1.0, 1.25, 2.0, 1.0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn difficulty_decodes_retail_columns_and_rejects_partial_records() {
        let bytes = retail_record();
        assert_eq!(bytes.len(), 168);
        let p = DifficultyParams::read_record(&mut Cursor::new(&bytes)).unwrap();
        assert_eq!(p.base_hp, [30, 45, 30, 24, 7, 24]);
        assert_eq!(p.hp_per_endurance, [5, 10, 5, 3, 3, 3]);
        assert_eq!(p.base_psi, [5, 10, 5, 3, 1, 5]);
        assert_eq!(p.psi_per_psionics, [10, 16, 10, 8, 5, 10]);
        assert_eq!(p.loot_discard_threshold, [0, 0, 0, 30, 75, 0]);
        assert_eq!(
            p.trainer_multiplier[Difficulty::Impossible.retail_index()],
            1.8
        );
        assert_eq!(
            p.replicator_multiplier[Difficulty::Hard.retail_index()],
            1.25
        );
        for len in 0..168 {
            assert!(DifficultyParams::read_record(&mut Cursor::new(&bytes[..len])).is_none());
        }
    }
    #[test]
    fn difficulty_reader_honors_chunk_bounds_and_missing_chunk() {
        let record = retail_record();
        for (name, length, expected) in [
            ("DIFFPARAM", 168_u32, true),
            ("DIFFPARAM", 167, false),
            ("OTHER", 168, false),
        ] {
            let toc_offset = 272 + 24 + record.len();
            let mut bytes = vec![0; 272 + 24];
            bytes[..4].copy_from_slice(&(toc_offset as u32).to_le_bytes());
            bytes.extend(&record);
            bytes.extend(1_u32.to_le_bytes());
            let mut chunk_name = [0; 12];
            chunk_name[..name.len()].copy_from_slice(name.as_bytes());
            bytes.extend(chunk_name);
            bytes.extend(272_u32.to_le_bytes());
            bytes.extend(length.to_le_bytes());
            let mut reader = Cursor::new(bytes);
            let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
            assert_eq!(
                DifficultyParams::read(&toc, &mut reader).is_some(),
                expected
            );
        }
    }

    #[test]
    fn difficulty_names_cannot_select_reserved_columns() {
        for (i, d) in Difficulty::ALL.into_iter().enumerate() {
            assert_eq!(d.retail_index(), i + 1);
            assert_eq!(d.label().parse(), Ok(d));
            assert_eq!(
                serde_json::from_str::<Difficulty>(&serde_json::to_string(&d).unwrap()).unwrap(),
                d
            );
        }
        for invalid in ["0", "5", "playtest", "multiplayer", "medium", ""] {
            assert!(invalid.parse::<Difficulty>().is_err());
        }
        assert!(serde_json::from_str::<Difficulty>("\"multiplayer\"").is_err());
    }
}

/// The first four STATPARAM fields, used when a DIFFPARAM coefficient is zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerPoolParams {
    pub base_hp: i32,
    pub hp_per_endurance: i32,
    pub base_psi: i32,
    pub psi_per_psionics: i32,
}
impl Default for PlayerPoolParams {
    fn default() -> Self {
        // Retail STATPARAM defaults, used only by synthetic/missing-data worlds.
        Self {
            base_hp: 30,
            hp_per_endurance: 5,
            base_psi: 20,
            psi_per_psionics: 5,
        }
    }
}
impl PlayerPoolParams {
    pub fn read<T: Read + Seek>(toc: &ChunkFileTableOfContents, reader: &mut T) -> Option<Self> {
        let chunk = toc.get_chunk("STATPARAM".to_owned())?;
        if chunk.length < 16 {
            return None;
        }
        reader.seek(io::SeekFrom::Start(chunk.offset)).ok()?;
        let mut values = [0; 4];
        reader.read_i32_into::<LittleEndian>(&mut values).ok()?;
        Some(Self {
            base_hp: values[0],
            hp_per_endurance: values[1],
            base_psi: values[2],
            psi_per_psionics: values[3],
        })
    }
}

#[cfg(test)]
mod pool_param_tests {
    use super::*;
    #[test]
    fn difficulty_pool_fallback_reads_statparam_header_not_hazard_floats() {
        let mut bytes = vec![0; 424];
        bytes[..4].copy_from_slice(&400_u32.to_le_bytes());
        bytes[400..404].copy_from_slice(&1_u32.to_le_bytes());
        bytes[404..413].copy_from_slice(b"STATPARAM");
        bytes[416..420].copy_from_slice(&280_u32.to_le_bytes());
        bytes[420..424].copy_from_slice(&56_u32.to_le_bytes());
        for (i, v) in [40_i32, 7, 12, 9].into_iter().enumerate() {
            bytes[304 + 4 * i..308 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        let mut reader = io::Cursor::new(bytes.clone());
        let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        assert_eq!(
            PlayerPoolParams::read(&toc, &mut reader),
            Some(PlayerPoolParams {
                base_hp: 40,
                hp_per_endurance: 7,
                base_psi: 12,
                psi_per_psionics: 9,
            })
        );
        bytes[420..424].copy_from_slice(&15_u32.to_le_bytes());
        let mut reader = io::Cursor::new(bytes);
        let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        assert!(PlayerPoolParams::read(&toc, &mut reader).is_none());
    }
}
