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
use dark::properties::{self, Link};
use dark::ss2_chunk_file_reader;
use dark::ss2_entity_info::{self, SystemShock2EntityInfo};

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

/// Parse a mission's entity info (properties + links) directly from its own
/// chunks, without merging the gamesys. Enough to assert on mission-local
/// properties and links (patrol data lives in the mission file).
fn load_mission_entity_info(data: &Path, mission: &str) -> SystemShock2EntityInfo {
    let file = File::open(data.join(mission)).expect("mission file should open");
    let mut reader = BufReader::new(file);
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
    let (props, links, links_with_data) = properties::get();
    ss2_entity_info::new(&toc, &links, &links_with_data, &props, &mut reader)
}

/// Count `Link::AIPatrol` edges across all of a mission's parsed links.
fn count_aipatrol_links(info: &SystemShock2EntityInfo) -> usize {
    info.template_to_links
        .values()
        .flat_map(|tl| &tl.to_links)
        .filter(|l| matches!(l.link, Link::AIPatrol))
        .count()
}

/// Count entities flagged as patrollers (`PropAIPatrol(true)`). Properties are
/// trait objects with no downcast, so match on the `Debug` form - the same
/// rendering `dark_query` shows.
fn count_patrolling_entities(info: &SystemShock2EntityInfo) -> usize {
    info.entity_to_properties
        .values()
        .filter(|props| {
            props
                .iter()
                .any(|p| format!("{p:?}").contains("PropAIPatrol(true)"))
        })
        .count()
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

#[test]
fn eng1_patrol_data_parses() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let info = load_mission_entity_info(&data, "eng1.mis");

    // eng1 has a substantial patrol network (a ~1.1KB L$AIPatrol chunk). The
    // link registration must turn those edges into typed Link::AIPatrol rather
    // than leaving them in the unparsed bucket.
    assert!(
        !info.unparsed_links.contains_key("L$AIPatrol"),
        "L$AIPatrol should be a registered (parsed) link, not unparsed"
    );
    // 81 route edges and 17 flagged patrollers is eng1's known, stable set; a
    // regression in the link/property parse shifts these counts.
    let patrol_links = count_aipatrol_links(&info);
    assert_eq!(
        patrol_links, 81,
        "eng1 parsed AIPatrol route-link count changed"
    );

    // ...and the AIs flagged to walk it (P$AI_Patrol = true).
    let patrollers = count_patrolling_entities(&info);
    assert_eq!(patrollers, 17, "eng1 PropAIPatrol(true) AI count changed");
}

#[test]
fn command1_tram_physical_attachments_parse() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let info = load_mission_entity_info(&data, "command1.mis");

    let mut attachments = info
        .template_to_links
        .iter()
        .flat_map(|(source, links)| {
            links
                .to_links
                .iter()
                .filter_map(move |link| match link.link {
                    Link::PhysAttach(options) => {
                        Some((*source, link.to_template_id, options.offset))
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(|(source, _, _)| *source);

    assert_eq!(
        attachments
            .iter()
            .map(|(source, _, _)| *source)
            .collect::<Vec<_>>(),
        vec![146, 152, 153, 155, 156, 199],
        "command1 tram should have five collision parts and its button physically attached"
    );
    assert!(
        attachments
            .iter()
            .all(|(_, destination, _)| *destination == 137),
        "every tram child should attach to root object 137"
    );
    let front = attachments
        .iter()
        .find(|(source, _, _)| *source == 152)
        .expect("Tram Front attachment should parse");
    assert_eq!(front.2, cgmath::vec3(4.0, -0.2, -0.075));
    assert!(
        !info.unparsed_links.contains_key("L$PhysAttac")
            && !info.unparsed_link_data.contains_key("LD$PhysAtta"),
        "the physical attachment relation and its payload must both be typed"
    );
}

#[test]
fn command1_tram_reroute_links_parse() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let info = load_mission_entity_info(&data, "command1.mis");

    let links_from = |source| {
        info.template_to_links
            .get(&source)
            .map(|links| {
                links
                    .to_links
                    .iter()
                    .map(|link| (link.to_template_id, link.link.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    for (button, waypoint) in [(686, 100), (722, 101), (723, 103)] {
        assert!(
            links_from(button).contains(&(waypoint, Link::ScriptParams)),
            "reroute button {button} should name waypoint {waypoint} with ScriptParams"
        );
    }
    assert!(
        links_from(137).contains(&(100, Link::ScriptParams)),
        "tram 137 should record waypoint 100 as its authored starting station"
    );
    assert!(
        links_from(137).contains(&(101, Link::TPathNext)),
        "tram 137 should initially target waypoint 101"
    );
    assert!(
        !info.unparsed_links.contains_key("L$ScriptPar")
            && !info.unparsed_links.contains_key("L$TPathNext"),
        "the authored tram routing relations must be typed: ScriptParams={:?}, TPathNext={:?}",
        info.unparsed_links.get("L$ScriptPar"),
        info.unparsed_links.get("L$TPathNext"),
    );
}

#[test]
fn medsci1_deaf_metaproperty_delivers_hearing_component() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };

    // Full pipeline: parse gamesys + mission, merge, and instantiate the
    // world - the same path the game takes - then assert the AI hearing
    // property lands on the right entities.
    let (props, links, links_with_data) = properties::get();
    let gam_file = File::open(data.join("shock2.gam")).expect("gamesys should open");
    let mut gam_reader = BufReader::new(gam_file);
    let gamesys = dark::gamesys::read(&mut gam_reader, &links, &links_with_data, &props);

    let mission_info = load_mission_entity_info(&data, "medsci1.mis");
    let merged = ss2_entity_info::merge_with_gamesys(&mission_info, &gamesys);

    let mut world = shipyard::World::new();
    let template_to_entity =
        merged
            .initialize_world_with_entities(&mut world, std::collections::HashMap::new(), |_| true);

    // Object 596 is an OG-Pipe hybrid with the `Deaf` metaproperty attached
    // (hearing rating 0); its sibling OG-Pipes 163 and 1007 hear normally
    // (no hearing property authored anywhere in their inheritance chain).
    let v_hearing = world
        .borrow::<shipyard::View<properties::PropAIHearing>>()
        .unwrap();
    let deaf = *template_to_entity
        .get(&596)
        .expect("medsci1 obj 596 (deaf OG-Pipe) should instantiate");
    let rating = shipyard::Get::get(&v_hearing, deaf)
        .expect("deaf OG-Pipe (obj 596) should inherit PropAIHearing from the Deaf metaproperty");
    assert_eq!(rating.rating, 0, "Deaf metaproperty should set rating 0");
    assert!(rating.is_deaf());

    for id in [163, 1007] {
        let entity = *template_to_entity
            .get(&id)
            .expect("medsci1 hearing OG-Pipes (obj 163/1007) should instantiate");
        assert!(
            shipyard::Get::get(&v_hearing, entity).is_err(),
            "OG-Pipe obj {id} should have no authored hearing property"
        );
    }
}

#[test]
fn gamesys_trainer_cost_tables_match_the_retail_values() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let file = File::open(data.join("shock2.gam")).expect("shock2.gam should open");
    let mut reader = BufReader::new(file);
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
    let costs = dark::gamesys::TrainerCostTables::read(&toc, &mut reader)
        .expect("retail shock2.gam carries the four cost chunks");

    // The retail tables (projects/flat-ui-panels.md §3.2; identical to the
    // community-documented values). Every stat row: 3/8/15/30/50.
    for row in &costs.stat_cost {
        assert_eq!(row, &[3, 8, 15, 30, 50]);
    }
    // Every tech-skill row: 10/5/8/12/25/50 (level 1 really costs more than 2).
    for row in &costs.tech_cost {
        assert_eq!(row, &[10, 5, 8, 12, 25, 50]);
    }
    // Every weapon-skill row: 12/6/8/15/36/50.
    for row in &costs.weapon_cost {
        assert_eq!(row, &[12, 6, 8, 15, 36, 50]);
    }
    // Psi tiers: first int = tier unlock (10/20/30/50/75), rest = per-power.
    let unlocks: Vec<i32> = costs.psi_cost.iter().map(|t| t[0]).collect();
    assert_eq!(unlocks, vec![10, 20, 30, 50, 75]);
    assert_eq!(costs.psi_cost[0][1..], [3; 7]);
    assert_eq!(costs.psi_cost[4][1..], [20; 7]);
}

#[test]
fn gamesys_hrm_params_match_retail_hacking_tuning() {
    let Some(data) = data_root() else {
        eprintln!("SKIP: no Data/ assets (set DARK_ASSET_PATH to run)");
        return;
    };
    let file = File::open(data.join("shock2.gam")).expect("shock2.gam should open");
    let mut reader = BufReader::new(file);
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
    let params = dark::gamesys::HrmParams::read(&toc, &mut reader)
        .expect("retail shock2.gam carries the 48-byte HRM chunk");

    assert_eq!(params.skill_critical_bonus, 0);
    assert_eq!(params.skill_success_bonus, 10);
    assert_eq!(params.stat_critical_bonus, 1);
    assert_eq!(params.stat_success_bonus, 5);
    assert_eq!(
        params.stat_break_chance,
        [10.0, 14.0, 20.0, 28.0, 38.0, 50.0, 75.0, 95.0]
    );

    // Earth starts at Hack 0 / Cyber 1. Its object-266 override (50,2)
    // therefore plays at 55% with one mine, not raw 50% / two mines.
    assert_eq!(params.success_chance(50, 0, 1), 55);
    assert_eq!(params.mine_count(2, 0, 1), 1);
}
