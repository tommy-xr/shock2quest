use anyhow::{Context, Result};
use dark::{
    gamesys::{self},
    properties::get,
    ss2_chunk_file_reader,
    ss2_entity_info::{self, merge_with_gamesys, SystemShock2EntityInfo},
};
use shock2vr::paths;
use std::{fs::File, io::BufReader};
use tracing::info;

/// Load the full gamesys (shock2.gam) including speech DB and sound schema
pub fn load_gamesys() -> Result<gamesys::Gamesys> {
    info!("Loading gamesys data from shock2.gam");

    let (properties, links, links_with_data) = get();

    // Load shock2.gam file
    let data_root = paths::data_root();
    let gam_path = data_root.join("shock2.gam");
    if !gam_path.exists() {
        return Err(anyhow::anyhow!(
            "shock2.gam not found. Checked these directories: {}",
            paths::search_roots().join(", ")
        ));
    }

    let game_file =
        File::open(&gam_path).with_context(|| format!("Failed to open {}", gam_path.display()))?;
    let mut game_reader = BufReader::new(game_file);

    let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

    info!(
        "Loaded {} entities from gamesys",
        gamesys.entity_info.entity_to_properties.len()
    );

    Ok(gamesys)
}

/// Load entity data from shock2.gam only
pub fn load_gamesys_only() -> Result<SystemShock2EntityInfo> {
    let gamesys = load_gamesys()?;
    Ok(gamesys.into_entity_info())
}

/// Load entity data from shock2.gam + specified mission file
pub fn load_gamesys_with_mission(mission_name: &str) -> Result<SystemShock2EntityInfo> {
    info!(
        "Loading gamesys + mission data from shock2.gam and {}",
        mission_name
    );

    let gamesys = load_gamesys()?;
    let data_root = paths::data_root();
    let (properties, links, links_with_data) = get();

    // Load mission file
    let mission_path = data_root.join(mission_name);
    if !mission_path.exists() {
        return Err(anyhow::anyhow!(
            "Mission file {} not found.",
            mission_path.display()
        ));
    }

    let mission_file = File::open(&mission_path)
        .with_context(|| format!("Failed to open {}", mission_path.display()))?;
    let mut mission_reader = BufReader::new(mission_file);

    // Read mission table of contents to get entity data chunks
    let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(&mut mission_reader);

    // Extract entity info directly without asset loading
    let mission_entity_info = ss2_entity_info::new(
        &table_of_contents,
        &links,
        &links_with_data,
        &properties,
        &mut mission_reader,
    );

    // Merge gamesys + mission data
    let merged_entity_info = merge_with_gamesys(&mission_entity_info, &gamesys);

    info!(
        "Loaded {} entities from mission",
        mission_entity_info.entity_to_properties.len()
    );
    info!(
        "Merged total: {} entities",
        merged_entity_info.entity_to_properties.len()
    );

    Ok(merged_entity_info)
}

/// Load entity data based on optional mission parameter
pub fn load_entity_data(mission: Option<&str>) -> Result<SystemShock2EntityInfo> {
    match mission {
        Some(mission_name) => load_gamesys_with_mission(mission_name),
        None => load_gamesys_only(),
    }
}
