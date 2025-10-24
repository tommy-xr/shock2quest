use anyhow::{Context, Result};
use std::{fs::File, io::BufReader, path::Path};
use tracing::info;

use dark::{
    gamesys::{self},
    ss2_entity_info::{SystemShock2EntityInfo},
    properties::get,
};

/// Load entity data from shock2.gam only
pub fn load_gamesys_only() -> Result<SystemShock2EntityInfo> {
    info!("Loading gamesys data from shock2.gam");

    let (properties, links, links_with_data) = get();

    // Load shock2.gam file
    let gam_path = "shock2.gam";
    if !Path::new(gam_path).exists() {
        return Err(anyhow::anyhow!("shock2.gam not found in current directory. Please run from the game data directory."));
    }

    let game_file = File::open(gam_path)
        .with_context(|| format!("Failed to open {}", gam_path))?;
    let mut game_reader = BufReader::new(game_file);

    let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

    info!("Loaded {} entities from gamesys", gamesys.entity_info.entity_to_properties.len());

    Ok(gamesys.entity_info)
}

/// Load entity data from shock2.gam + specified mission file
/// TODO: Mission loading is not yet implemented - for now just returns gamesys data
pub fn load_gamesys_with_mission(mission_name: &str) -> Result<SystemShock2EntityInfo> {
    info!("Mission loading not yet implemented, loading gamesys only (mission: {})", mission_name);
    load_gamesys_only()
}

/// Load entity data based on optional mission parameter
pub fn load_entity_data(mission: Option<&str>) -> Result<SystemShock2EntityInfo> {
    match mission {
        Some(mission_name) => load_gamesys_with_mission(mission_name),
        None => load_gamesys_only(),
    }
}