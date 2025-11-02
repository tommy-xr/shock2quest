use std::io::Read;

use super::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

fn import_text(
    _name: String,
    reader: &mut Box<dyn super::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> String {
    let mut content = String::new();
    let _ = reader.read_to_string(&mut content);
    content
}

fn process_text(content: String, _asset_cache: &mut AssetCache, _config: &()) -> String {
    content.trim().to_string()
}

pub static TEXT_IMPORTER: Lazy<AssetImporter<String, String, ()>> =
    Lazy::new(|| AssetImporter::define(import_text, process_text));
