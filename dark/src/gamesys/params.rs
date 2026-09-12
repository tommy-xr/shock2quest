//! Trainer upgrade cost tables, read from the gamesys chunk file-vars.
//!
//! The retail engine stores the trainer costs as gamesys "file-var" chunks
//! with these layouts: `STATCOST` (5 stats x 5 levels), `WTECHCOST`
//! (5 tech skills x 6 levels), `WSKILLCOST` (4 weapon skills x 6 levels) and
//! `PSICOST` (5 tiers x 8 ints, first int of each tier row = the tier unlock
//! cost, the remaining 7 = per-power costs within that tier). All values are
//! little-endian `i32`, Normal difficulty (the per-difficulty multipliers live
//! in `DIFFPARAM`, whose exact layout is unverified - Normal-only for now).
//!
//! Decoded from the shipped `shock2.gam` (projects/flat-ui-panels.md §3.2) and
//! identical to the community-documented tables, so both sources corroborate.

use std::io;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::{ss2_chunk_file_reader::ChunkFileTableOfContents, ss2_common::DEGREES_PER_ANGLE_UNIT};

/// The trainer upgrade cost tables (Normal difficulty), as authored in the
/// gamesys. Each row is one stat/skill/tier; each column is the cost of buying
/// the *next* level from that column's level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainerCostTables {
    /// `STATCOST[5 stats][5]`: cost of raising a stat from level `i+1` to
    /// `i+2` (stats start at 1, cap 6).
    pub stat_cost: [[i32; 5]; 5],
    /// `WTECHCOST[5 tech skills][6]`: cost of raising a tech skill from level
    /// `i` to `i+1` (skills start at 0, cap 6).
    pub tech_cost: [[i32; 6]; 5],
    /// `WSKILLCOST[4 weapon skills][6]`: cost of raising a weapon skill from
    /// level `i` to `i+1` (skills start at 0, cap 6).
    pub weapon_cost: [[i32; 6]; 4],
    /// `PSICOST[5 tiers][8]`: per tier, `[0]` = the tier unlock cost, `[1..8]`
    /// = the per-power costs within that tier.
    pub psi_cost: [[i32; 8]; 5],
}

/// Retail `HRM` gamesys file-var (`sHRMParams`): bonuses applied to an
/// object's `P$HackDiff` base values for the player's relevant tech skill and
/// Cyber Affinity stat.
#[derive(Debug, Clone, PartialEq)]
pub struct HrmParams {
    pub skill_critical_bonus: i32,
    pub skill_success_bonus: i32,
    pub stat_critical_bonus: i32,
    pub stat_success_bonus: i32,
    /// Authored `m_statBreak` table for Cyber Affinity 1..=8. Retail exposes
    /// this data in the editor but `ShockHRMTriggerEffect` does not consult it:
    /// landing on a failed mine breaks a hack target unconditionally.
    pub stat_break_chance: [f32; 8],
}

/// Retail `SKILLPARAM` file-var (`sSkillParams`). Research scales authored
/// progress by `1 + research_factor * (skill - 1)^2`.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillParams {
    /// Shot deviation per level of weapon skill missing, in degrees. The
    /// shipped gamesys authors 0, so retail shots never deviate.
    pub inaccuracy_degrees: f32,
    pub weapon_break_factor: f32,
    pub research_factor: f32,
    pub damage_modifier: f32,
    pub organ_damage: f32,
}

fn read_table<T: io::Read + io::Seek, const COLS: usize, const ROWS: usize>(
    table_of_contents: &ChunkFileTableOfContents,
    reader: &mut T,
    chunk_name: &str,
) -> Option<[[i32; COLS]; ROWS]> {
    let chunk = table_of_contents.get_chunk(chunk_name.to_owned())?;
    // A truncated chunk must fail, not silently read into the next chunk's
    // bytes. (TOC lengths exclude the 24-byte header; the retail chunks are
    // exactly ROWS*COLS i32s: STATCOST 100, WTECHCOST 120, WSKILLCOST 96,
    // PSICOST 160.)
    if (chunk.length as usize) < ROWS * COLS * std::mem::size_of::<i32>() {
        return None;
    }
    reader.seek(io::SeekFrom::Start(chunk.offset)).ok()?;
    let mut table = [[0i32; COLS]; ROWS];
    for row in table.iter_mut() {
        for cell in row.iter_mut() {
            *cell = reader.read_i32::<LittleEndian>().ok()?;
        }
    }
    Some(table)
}

impl TrainerCostTables {
    /// Read the four cost chunks from a gamesys chunk file. Returns `None` if
    /// any chunk is missing or truncated (a non-retail gamesys).
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<TrainerCostTables> {
        Some(TrainerCostTables {
            stat_cost: read_table(table_of_contents, reader, "STATCOST")?,
            tech_cost: read_table(table_of_contents, reader, "WTECHCOST")?,
            weapon_cost: read_table(table_of_contents, reader, "WSKILLCOST")?,
            psi_cost: read_table(table_of_contents, reader, "PSICOST")?,
        })
    }
}

impl HrmParams {
    /// Read the 48-byte `HRM` chunk. Returns `None` for a missing/truncated
    /// gamesys rather than reading through into the next chunk.
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<Self> {
        let chunk = table_of_contents.get_chunk("HRM".to_owned())?;
        if chunk.length < 48 {
            return None;
        }
        reader.seek(io::SeekFrom::Start(chunk.offset)).ok()?;
        let skill_critical_bonus = reader.read_i32::<LittleEndian>().ok()?;
        let skill_success_bonus = reader.read_i32::<LittleEndian>().ok()?;
        let stat_critical_bonus = reader.read_i32::<LittleEndian>().ok()?;
        let stat_success_bonus = reader.read_i32::<LittleEndian>().ok()?;
        let mut stat_break_chance = [0.0; 8];
        for chance in &mut stat_break_chance {
            *chance = reader.read_f32::<LittleEndian>().ok()?;
        }
        Some(Self {
            skill_critical_bonus,
            skill_success_bonus,
            stat_critical_bonus,
            stat_success_bonus,
            stat_break_chance,
        })
    }

    pub fn success_chance(&self, base: i32, skill: i32, stat: i32) -> i32 {
        (base + skill * self.skill_success_bonus + stat * self.stat_success_bonus).min(85)
    }

    pub fn mine_count(&self, base: i32, skill: i32, stat: i32) -> i32 {
        (base - skill * self.skill_critical_bonus - stat * self.stat_critical_bonus).max(0)
    }
}

impl SkillParams {
    /// Read the packed 18-byte `SKILLPARAM` chunk. Returns `None` for a
    /// missing/truncated non-retail gamesys.
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<Self> {
        let chunk = table_of_contents.get_chunk("SKILLPARAM".to_owned())?;
        if chunk.length < 18 {
            return None;
        }
        reader.seek(io::SeekFrom::Start(chunk.offset)).ok()?;
        Self::read_record(reader)
    }

    fn read_record<T: io::Read>(reader: &mut T) -> Option<Self> {
        Some(Self {
            inaccuracy_degrees: reader.read_u16::<LittleEndian>().ok()? as f32
                * DEGREES_PER_ANGLE_UNIT,
            weapon_break_factor: reader.read_f32::<LittleEndian>().ok()?,
            research_factor: reader.read_f32::<LittleEndian>().ok()?,
            damage_modifier: reader.read_f32::<LittleEndian>().ok()?,
            organ_damage: reader.read_f32::<LittleEndian>().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::SkillParams;

    #[test]
    fn parses_packed_retail_skill_params_layout() {
        // The shipped gamesys record: inaccuracy 0, then the four floats.
        let mut bytes = 0_u16.to_le_bytes().to_vec();
        for value in [0.0_f32, 1.0, 0.15, 1.25] {
            bytes.extend(value.to_le_bytes());
        }
        assert_eq!(bytes.len(), 18);

        let parsed = SkillParams::read_record(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(parsed.inaccuracy_degrees, 0.0);
        assert_eq!(parsed.weapon_break_factor, 0.0);
        assert_eq!(parsed.research_factor, 1.0);
        assert!((parsed.damage_modifier - 0.15).abs() < f32::EPSILON);
        assert_eq!(parsed.organ_damage, 1.25);
    }

    /// The leading field is a 16-bit turn, not a raw count of degrees.
    #[test]
    fn inaccuracy_converts_from_sixteen_bit_turns() {
        let mut bytes = 2048_u16.to_le_bytes().to_vec();
        bytes.extend([0u8; 16]);

        let parsed = SkillParams::read_record(&mut Cursor::new(bytes)).unwrap();

        assert_eq!(parsed.inaccuracy_degrees, 11.25);
    }
}

/// Endurance damage multipliers from STATPARAM (six header fields, eight floats).
#[derive(Debug, Clone)]
pub struct HazardParams(pub [f32; 8]);
impl HazardParams {
    pub fn read<T: io::Read + io::Seek>(
        toc: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> Option<Self> {
        let chunk = toc.get_chunk("STATPARAM".to_owned())?;
        if chunk.length < 56 {
            return None;
        }
        reader.seek(io::SeekFrom::Start(chunk.offset + 24)).ok()?;
        let mut values = [0.0; 8];
        for value in &mut values {
            *value = reader.read_f32::<LittleEndian>().ok()?;
            if !value.is_finite() || *value < 0.0 {
                return None;
            }
        }
        Some(Self(values))
    }
}

#[cfg(test)]
mod hazard_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn hazard_table_follows_six_stat_fields_and_respects_chunk_bounds() {
        // Minimal chunk file. sStatParams is four ints, two floats, eight
        // hazard floats; the extra 24 bytes here are NOT a chunk header.
        let mut bytes = vec![0_u8; 512];
        bytes[..4].copy_from_slice(&400_u32.to_le_bytes());
        bytes[400..404].copy_from_slice(&1_u32.to_le_bytes());
        bytes[404..413].copy_from_slice(b"STATPARAM");
        bytes[416..420].copy_from_slice(&280_u32.to_le_bytes());
        bytes[420..424].copy_from_slice(&56_u32.to_le_bytes());
        for (i, value) in [30_i32, 5, 20, 5, 0, 0].into_iter().enumerate() {
            bytes[304 + 4 * i..308 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        let expected = [1.0_f32, 0.94, 0.85, 0.73, 0.58, 0.4, 0.2, 0.01];
        for (i, value) in expected.iter().enumerate() {
            bytes[328 + 4 * i..332 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        let mut reader = Cursor::new(bytes.clone());
        let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        assert_eq!(HazardParams::read(&toc, &mut reader).unwrap().0, expected);
        bytes[420..424].copy_from_slice(&52_u32.to_le_bytes());
        let mut reader = Cursor::new(bytes);
        let toc = crate::ss2_chunk_file_reader::read_table_of_contents(&mut reader);
        assert!(HazardParams::read(&toc, &mut reader).is_none());
    }
}
