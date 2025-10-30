use std::io::{self, Read};

use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::map::MapRect;

/// Raw binary data for map position files (P001RA.BIN, P001XA.BIN)
#[derive(Clone, Debug)]
pub struct RawMapPositionData {
    pub bytes: Vec<u8>,
}

/// Configuration for map position loading (currently no options needed)
#[derive(Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct MapPositionOptions {}

pub(crate) fn load_map_position(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &MapPositionOptions,
) -> RawMapPositionData {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    RawMapPositionData { bytes: buf }
}

pub(crate) fn process_map_position(
    raw_data: RawMapPositionData,
    _assets: &mut AssetCache,
    _config: &MapPositionOptions,
) -> Vec<MapRect> {
    parse_map_rects(&raw_data.bytes).unwrap_or_else(|_| Vec::new())
}

/// Parse rectangle data from binary buffer
fn parse_map_rects(bytes: &[u8]) -> io::Result<Vec<MapRect>> {
    let mut rects = Vec::new();
    let mut offset = 0;

    while offset + 8 <= bytes.len() {
        // Parse as little-endian 16-bit integers
        let ul_x = i16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let ul_y = i16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
        let lr_x = i16::from_le_bytes([bytes[offset + 4], bytes[offset + 5]]);
        let lr_y = i16::from_le_bytes([bytes[offset + 6], bytes[offset + 7]]);

        rects.push(MapRect::new(ul_x, ul_y, lr_x, lr_y));
        offset += 8;
    }

    Ok(rects)
}

pub static MAP_POSITION_IMPORTER: Lazy<
    AssetImporter<RawMapPositionData, Vec<MapRect>, MapPositionOptions>,
> = Lazy::new(|| AssetImporter::define(load_map_position, process_map_position));
