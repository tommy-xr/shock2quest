use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::audio::Song;

pub static SONG_IMPORTER: Lazy<AssetImporter<Song, Song, ()>> =
    Lazy::new(|| AssetImporter::define(load_song, |song, _cache, _config| song));

fn load_song(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Song {
    Song::read(reader)
}
