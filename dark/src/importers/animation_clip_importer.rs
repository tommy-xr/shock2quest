use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::motion::{AnimationClip, MotionClip};

use super::MOTIONDB_IMPORTER;

fn import_animation_cliip(
    name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    assets: &mut AssetCache,
    _config: &(),
) -> AnimationClip {
    let motiondb = assets.get(&MOTIONDB_IMPORTER, "motiondb.bin");

    // To look up in motion db, we need to remove the extension "_.mc" (4 characters):
    let name_without_extra_stuff = &name[..name.len() - 4];

    let mps_motion = motiondb.get_mps_motions(name_without_extra_stuff.to_owned());
    let motion_stuff = motiondb.get_motion_stuff(name_without_extra_stuff.to_owned());
    let motion_clip = MotionClip::read(reader, mps_motion);

    AnimationClip::create(&motion_clip, mps_motion, motion_stuff)
}

fn process_animation_clip(
    content: AnimationClip,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> AnimationClip {
    content
}

pub static ANIMATION_CLIP_IMPORTER: Lazy<AssetImporter<AnimationClip, AnimationClip, ()>> =
    Lazy::new(|| AssetImporter::define(import_animation_cliip, process_animation_clip));
