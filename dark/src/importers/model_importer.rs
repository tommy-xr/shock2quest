use std::io::{Read, Seek};
use std::{path::PathBuf, rc::Rc};

use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::{
    ss2_bin_ai_loader::{self, SystemShock2AIMesh},
    ss2_bin_header,
    ss2_bin_obj_loader::{self, SystemShock2ObjectMesh},
    ss2_skeleton::Skeleton,
};

use crate::model::Model;

use super::skeleton_importer::SKELETON_IMPORTER;

// Model importer

pub enum SystemShockContentModel {
    /// The optional third field is the 25AE high-detail `PMNM` chunk appended to
    /// the file, when present and enabled.
    Mesh(
        SystemShock2AIMesh,
        Rc<Skeleton>,
        Option<crate::ss2_bin_pmnm::PmnmMesh>,
    ),
    Obj(SystemShock2ObjectMesh),
}

fn load_model(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> SystemShockContentModel {
    let common_header = ss2_bin_header::read(reader);
    match common_header.bin_type {
        ss2_bin_header::BinFileType::Obj => {
            SystemShockContentModel::Obj(ss2_bin_obj_loader::read(reader, &common_header))
        }
        ss2_bin_header::BinFileType::Mesh => {
            let mut pathbuf = PathBuf::from(_name);
            pathbuf.set_extension("cal");
            let cal_path = pathbuf.to_string_lossy();
            let skeleton = _assets.get(&SKELETON_IMPORTER, &cal_path);
            let ai_mesh = ss2_bin_ai_loader::read(reader, &common_header);

            // The high-detail chunk is appended past the LGMM data, so it needs
            // the whole file rather than the streaming reader.
            let pmnm = if ss2_bin_ai_loader::pmnm_enabled() {
                let mut buf = Vec::new();
                reader
                    .seek(std::io::SeekFrom::Start(0))
                    .and_then(|_| reader.read_to_end(&mut buf))
                    .ok()
                    .and_then(|_| crate::ss2_bin_pmnm::find_chunk(&buf))
                    .and_then(|base| crate::ss2_bin_pmnm::read(&buf, base))
            } else {
                None
            };

            SystemShockContentModel::Mesh(ai_mesh, skeleton, pmnm)
        }
    }
}

fn process_model(
    mesh: SystemShockContentModel,
    asset_cache: &mut AssetCache,
    _config: &(),
) -> Model {
    match mesh {
        SystemShockContentModel::Obj(obj) => Model::from_obj_bin(obj, asset_cache),
        SystemShockContentModel::Mesh(mesh, skeleton, pmnm) => {
            Model::from_ai_bin(mesh, skeleton, pmnm, asset_cache)
        }
    }
}

pub static MODELS_IMPORTER: Lazy<AssetImporter<SystemShockContentModel, Model, ()>> =
    Lazy::new(|| AssetImporter::define(load_model, process_model));
