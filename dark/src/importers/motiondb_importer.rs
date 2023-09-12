use std::rc::Rc;

use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::motion::MotionDB;

fn import_motion_db(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Rc<MotionDB> {
    Rc::new(MotionDB::read(reader))
}

fn process_motion_db(
    content: Rc<MotionDB>,
    _asset_cache: &mut AssetCache,
    _config: &(),
) -> Rc<MotionDB> {
    content
}

pub static MOTIONDB_IMPORTER: Lazy<AssetImporter<Rc<MotionDB>, Rc<MotionDB>, ()>> =
    Lazy::new(|| AssetImporter::define(import_motion_db, process_motion_db));
