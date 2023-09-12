use std::path::Path;

use engine::{
    assets::{asset_cache::AssetCache, asset_importer::AssetImporter},
    texture::{self, Texture, TextureOptions},
    texture_format::RawTextureData,
};
use once_cell::sync::Lazy;

pub(crate) fn load_texture(
    name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &TextureOptions,
) -> RawTextureData {
    let extension = Path::new(&name).extension().unwrap();
    let format =
        engine::texture_format::extension_to_format(extension.to_str().unwrap().to_string())
            .unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    format.load(&buf)
}

pub(crate) fn process_texture(
    raw_texture_data: RawTextureData,
    _assets: &mut AssetCache,
    config: &TextureOptions,
) -> Texture {
    texture::init_from_memory2(raw_texture_data, config)
}

pub static TEXTURE_IMPORTER: Lazy<AssetImporter<RawTextureData, Texture, TextureOptions>> =
    Lazy::new(|| AssetImporter::define(load_texture, process_texture));
