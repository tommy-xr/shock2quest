use engine::{
    assets::{asset_cache::AssetCache, asset_importer::AssetImporter},
    audio::AudioClip,
};
use once_cell::sync::Lazy;

pub static AUDIO_IMPORTER: Lazy<AssetImporter<AudioClip, AudioClip, ()>> =
    Lazy::new(|| AssetImporter::define(load_audio, |audio, _cache, _config| audio));

fn load_audio(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> AudioClip {
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);

    AudioClip::from_bytes(buf)
}
