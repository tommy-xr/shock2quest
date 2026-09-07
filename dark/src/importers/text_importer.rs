//! Loader for authored text data files (JSON), read through the same mount
//! stack as every other asset so a file that ships in the bundle resolves on
//! device as well as off it.

use std::io::Read;

use engine::assets::{
    asset_cache::AssetCache, asset_importer::AssetImporter, asset_paths::ReadableAndSeekable,
};
use once_cell::sync::Lazy;

/// A text asset's contents, lossily decoded as UTF-8.
#[derive(Clone, Debug)]
pub struct TextAsset(pub String);

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct TextOptions {}

pub(crate) fn load_text(
    _name: String,
    reader: &mut Box<dyn ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &TextOptions,
) -> TextAsset {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    TextAsset(String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn process_text(
    raw: TextAsset,
    _assets: &mut AssetCache,
    _config: &TextOptions,
) -> TextAsset {
    raw
}

pub static TEXT_IMPORTER: Lazy<AssetImporter<TextAsset, TextAsset, TextOptions>> =
    Lazy::new(|| AssetImporter::define(load_text, process_text));
