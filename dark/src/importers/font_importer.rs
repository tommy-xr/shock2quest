use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::font::Font;

pub static FONT_IMPORTER: Lazy<AssetImporter<Box<dyn engine::Font>, Box<dyn engine::Font>, ()>> =
    Lazy::new(|| AssetImporter::define(load_font, |font, _cache, _config| font));

fn load_font(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Box<dyn engine::Font> {
    Box::new(Font::read(reader))
}
