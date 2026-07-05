//! Integration tests that load real mission files and assert on the parsed
//! structures - the middle layer between pure unit tests (no game data) and
//! the SDK e2e suite (spawns a full game with a GPU/HTTP runtime).
//!
//! These load and parse `.mis` chunks directly (no rendering, no game loop),
//! so they are fast and can assert on internal parse results the e2e can't
//! reach. They need the game assets in `Data/`; when those aren't present
//! (e.g. a CI runner without copyrighted assets) each test skips cleanly
//! rather than failing, so `cargo test` stays green everywhere.
//!
//! Point them at assets with `DARK_ASSET_PATH=/path/to/Data`, or run from a
//! checkout that has `Data/` beside the workspace root.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use dark::mission::PathDatabase;
use dark::ss2_chunk_file_reader;

/// Locate the `Data/` directory, or `None` when assets aren't available.
///
/// Checks `DARK_ASSET_PATH` first, then walks up from the crate directory -
/// tests run with the crate root as the working directory, and `Data/` lives
/// beside the workspace root one level up.
fn data_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DARK_ASSET_PATH") {
        let p = PathBuf::from(path);
        if p.join("medsci1.mis").exists() {
            return Some(p);
        }
    }
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in ["../Data", "../../Data", "Data"] {
        let p = base.join(candidate);
        if p.join("medsci1.mis").exists() {
            return Some(p);
        }
    }
    None
}

/// Load a mission's AIPATH pathfinding database, or `None` if the mission has
/// no usable AIPATH chunk.
fn load_path_database(data: &Path, mission: &str) -> Option<PathDatabase> {
    let file = File::open(data.join(mission)).expect("mission file should open");
    let mut reader = BufReader::new(file);
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
    PathDatabase::read(&toc, &mut reader)
}

/// Every `.mis` filename in `data`.
fn all_missions(data: &Path) -> Vec<String> {
    std::fs::read_dir(data)
        .expect("Data directory should read")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".mis"))
        .collect()
}

#[test]
fn medsci1_cell_door_table_parses() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let db = load_path_database(&data, "medsci1.mis").expect("medsci1 has AIPATH data");

    // The cell-door table maps below-door path cells to the door object that
    // gates them. medsci1 has a known, stable set (532 cell entries across
    // 56 doors) - a regression in the trailing-section parse changes this.
    assert_eq!(
        db.cell_doors.len(),
        532,
        "medsci1 cell-door entry count changed"
    );
    let unique_doors: std::collections::HashSet<i32> =
        db.cell_doors.iter().map(|cd| cd.door).collect();
    assert_eq!(unique_doors.len(), 56, "medsci1 unique door count changed");

    // Object 1729 is "Sci Med Door" - it must appear in the table, and every
    // mapped cell must be a valid index into the cell array.
    assert!(
        unique_doors.contains(&1729),
        "known Sci Med Door (obj 1729) missing from the cell-door table"
    );
    for cd in &db.cell_doors {
        assert!(
            (cd.cell as usize) < db.cells.len(),
            "cell-door maps cell {} out of range ({} cells)",
            cd.cell,
            db.cells.len()
        );
    }
}

#[test]
fn all_missions_load_with_consistent_aipath() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };

    let mut missions_with_doors = 0;
    for mission in all_missions(&data) {
        // shodan.mis is a known bad file (issue #267); don't fail on it.
        if mission == "shodan.mis" {
            continue;
        }
        let Some(db) = load_path_database(&data, &mission) else {
            continue; // no AIPATH chunk - fine
        };

        // A misaligned trailing-section parse would produce cell ids past the
        // end of the cell array; this catches it across every mission.
        for cd in &db.cell_doors {
            assert!(
                (cd.cell as usize) < db.cells.len(),
                "{mission}: cell-door maps cell {} out of range ({} cells)",
                cd.cell,
                db.cells.len()
            );
        }
        if !db.cell_doors.is_empty() {
            missions_with_doors += 1;
        }
    }

    assert!(
        missions_with_doors >= 15,
        "expected most missions to have a parsed cell-door table, got {missions_with_doors}"
    );
}

#[test]
fn shodan_v34_aipath_loads_without_door_data() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    // shodan.mis is the only v3.4 (WideIds) AIPATH. Its trailing layout isn't
    // reverse-engineered, so the tail parse is intentionally not attempted -
    // the database still loads, just with no cell-door table (never a
    // silently-misparsed one).
    if let Some(db) = load_path_database(&data, "shodan.mis") {
        assert!(
            db.cell_doors.is_empty(),
            "v3.4 tail must not be parsed until its layout is verified"
        );
    }
}
