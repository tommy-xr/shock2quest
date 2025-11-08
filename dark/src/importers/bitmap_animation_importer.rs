use std::{path::Path, rc::Rc};

use engine::{
    assets::{asset_cache::AssetCache, asset_importer::AssetImporter},
    texture::{self, Texture, TextureOptions},
};
use once_cell::sync::Lazy;

use crate::{BitmapAnimation, importers::load_texture, util::load_multiple_textures_for_model};

fn load_bitmap_animation(
    name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    assets: &mut AssetCache,
    _config: &(),
) -> Vec<Rc<Texture>> {
    let config = TextureOptions::default();
    let raw_texture = load_texture(name.to_owned(), reader, assets, &config);
    let texture = Rc::new(texture::init_from_memory2(raw_texture, &config));
    let _path = Path::new(&name);

    let additional_frames = load_multiple_textures_for_model(assets, &name);

    let mut initial = vec![texture];
    initial.extend(additional_frames);
    initial
}

fn process_bitmap_animation(
    raw_texture_data: Vec<Rc<Texture>>,
    _assets: &mut AssetCache,
    _config: &(),
) -> BitmapAnimation {
    BitmapAnimation::new(raw_texture_data)
}

pub static BITMAP_ANIMATION_IMPORTER: Lazy<AssetImporter<Vec<Rc<Texture>>, BitmapAnimation, ()>> =
    Lazy::new(|| AssetImporter::define(load_bitmap_animation, process_bitmap_animation));
