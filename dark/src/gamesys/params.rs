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

use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;

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
