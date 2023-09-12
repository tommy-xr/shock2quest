use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::{
    ss2_cal_loader::{self, SystemShock2Cal},
    ss2_skeleton::{self, Skeleton},
};

fn load_skeleton(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> SystemShock2Cal {
    ss2_cal_loader::read(reader)
}

fn process_skeleton(
    content: SystemShock2Cal,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> Skeleton {
    ss2_skeleton::create(content)
}

pub static SKELETON_IMPORTER: Lazy<AssetImporter<SystemShock2Cal, Skeleton, ()>> =
    Lazy::new(|| AssetImporter::define(load_skeleton, process_skeleton));
