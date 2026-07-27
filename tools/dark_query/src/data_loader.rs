use anyhow::{Context, Result};
use dark::{
    gamesys::{self},
    properties::get,
    ss2_chunk_file_reader,
    ss2_entity_info::{self, SystemShock2EntityInfo, merge_with_gamesys},
};
use engine::assets::asset_paths::{AbstractAssetPath, ReadableAndSeekable};
use shock2vr::{data_files, paths};
use std::cell::RefCell;
use std::sync::OnceLock;
use tracing::info;

/// The data-file mounts, built once per process: indexing a 25th Anniversary
/// archive is not free, and both the gamesys and the mission go through here.
fn data_file_paths() -> &'static dyn AbstractAssetPath {
    static PATHS: OnceLock<Box<dyn AbstractAssetPath>> = OnceLock::new();
    &**PATHS.get_or_init(|| data_files::asset_paths(paths::data_root()))
}

/// Open a raw data file (the gamesys, a mission, `motiondb.bin`) through the
/// same asset-path layer the game uses, so a 25th Anniversary install - where
/// nothing is loose on disk and everything lives inside `sshock2.kpf` - works
/// just like a classic one.
pub fn open_data_file(name: &str) -> Result<RefCell<Box<dyn ReadableAndSeekable>>> {
    let data_root = paths::data_root();
    data_file_paths()
        .get_reader(
            data_root.to_string_lossy().into_owned(),
            name.to_ascii_lowercase(),
        )
        .with_context(|| {
            format!(
                "{name} not found in the game data at {} - set DARK_ASSET_PATH to your \
                 System Shock 2 install if that is the wrong directory",
                data_root.display()
            )
        })
}

/// Load the full gamesys (shock2.gam) including speech DB and sound schema
pub fn load_gamesys() -> Result<gamesys::Gamesys> {
    info!("Loading gamesys data from shock2.gam");

    let (properties, links, links_with_data) = get();

    let game_reader = open_data_file("shock2.gam")?;

    let gamesys = gamesys::read(
        &mut *game_reader.borrow_mut(),
        &links,
        &links_with_data,
        &properties,
    );

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
    let (properties, links, links_with_data) = get();

    // Load mission file
    let mission_reader = open_data_file(mission_name)?;
    let mut mission_reader = mission_reader.borrow_mut();

    // Read mission table of contents to get entity data chunks
    let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(&mut *mission_reader);

    // Extract entity info directly without asset loading
    let mission_entity_info = ss2_entity_info::new(
        &table_of_contents,
        &links,
        &links_with_data,
        &properties,
        &mut *mission_reader,
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
