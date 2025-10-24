use anyhow::{Context, Result};
use std::{fs::File, io::BufReader, path::Path};
use tracing::info;
use dark::{
    gamesys::{self},
    ss2_entity_info::{self, SystemShock2EntityInfo, merge_with_gamesys},
    ss2_chunk_file_reader,
    properties::get,
};

// For CLI tools, detect if we're running from Data directory or from tools/dark_query
fn get_base_path() -> String {
    if std::path::Path::new("shock2.gam").exists() {
        // Running from Data directory
        ".".to_string()
    } else if std::path::Path::new("../../Data/shock2.gam").exists() {
        // Running from tools/dark_query directory
        "../../Data".to_string()
    } else {
        // Default to current directory and let it fail with a helpful error
        ".".to_string()
    }
}


/// Load entity data from shock2.gam only
pub fn load_gamesys_only() -> Result<SystemShock2EntityInfo> {
    info!("Loading gamesys data from shock2.gam");

    let (properties, links, links_with_data) = get();

    // Load shock2.gam file
    let base_path = get_base_path();
    let gam_path = format!("{}/shock2.gam", base_path);
    if !Path::new(&gam_path).exists() {
        return Err(anyhow::anyhow!("shock2.gam not found. Please run from the Data directory or from tools/dark_query."));
    }

    let game_file = File::open(&gam_path)
        .with_context(|| format!("Failed to open {}", gam_path))?;
    let mut game_reader = BufReader::new(game_file);

    let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

    info!("Loaded {} entities from gamesys", gamesys.entity_info.entity_to_properties.len());

    Ok(gamesys.entity_info)
}

/// Load entity data from shock2.gam + specified mission file
pub fn load_gamesys_with_mission(mission_name: &str) -> Result<SystemShock2EntityInfo> {
    info!("Loading gamesys + mission data from shock2.gam and {}", mission_name);

    let (properties, links, links_with_data) = get();

    // Load shock2.gam file
    let base_path = get_base_path();
    let gam_path = format!("{}/shock2.gam", base_path);
    if !Path::new(&gam_path).exists() {
        return Err(anyhow::anyhow!("shock2.gam not found. Please run from the Data directory or from tools/dark_query."));
    }

    let game_file = File::open(&gam_path)
        .with_context(|| format!("Failed to open {}", gam_path))?;
    let mut game_reader = BufReader::new(game_file);

    let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

    // Load mission file
    let mission_path = format!("{}/{}", base_path, mission_name);
    if !Path::new(&mission_path).exists() {
        return Err(anyhow::anyhow!("Mission file {} not found.", mission_path));
    }

    let mission_file = File::open(&mission_path)
        .with_context(|| format!("Failed to open {}", mission_path))?;
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

    info!("Loaded {} entities from gamesys", gamesys.entity_info.entity_to_properties.len());
    info!("Loaded {} entities from mission", mission_entity_info.entity_to_properties.len());
    info!("Merged total: {} entities", merged_entity_info.entity_to_properties.len());

    Ok(merged_entity_info)
}

/// Load entity data based on optional mission parameter
pub fn load_entity_data(mission: Option<&str>) -> Result<SystemShock2EntityInfo> {
    match mission {
        Some(mission_name) => load_gamesys_with_mission(mission_name),
        None => load_gamesys_only(),
    }
}