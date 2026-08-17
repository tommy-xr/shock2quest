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
            // the whole file rather than the streaming reader. The reader is
            // sitting at the end of the LGMM data now, which is where the scan
            // should start - searching from 0 would also search the vertex and
            // index bytes.
            let pmnm = if ss2_bin_ai_loader::pmnm_enabled() {
                let lgmm_end = reader.stream_position().unwrap_or(0) as usize;
                let mut buf = Vec::new();
                reader
                    .seek(std::io::SeekFrom::Start(0))
                    .and_then(|_| reader.read_to_end(&mut buf))
                    .ok()
                    .and_then(|_| crate::ss2_bin_pmnm::find_chunk(&buf, lgmm_end))
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

/// Whether `name` is a material the 25th Anniversary Edition draws the player's
/// hand and forearm with on a first-person weapon model (`obj/*_h.bin`).
///
/// The remaster gives the arm its own material - `ND-arm.psd` on most models,
/// `ND-arm_atek.psd` on the pistol and shotgun - which is the only selector
/// that isolates it everywhere. A sub-object filter works on `ar15_h`/`atek_h`,
/// where the hand is its own `@s01_han`/`@s02_han`, but not on `sg_h` or
/// `empgun_h`, where the hand rides a moving gun sub-object.
pub fn is_first_person_arm_material(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("nd-arm")
}

/// A first-person weapon model prepared for VR wielding, as its own cache
/// bucket (see [`FirstPersonHand`] for the type_id trap).
///
/// The 25AE `obj/*_h` models bake in the player's hand and forearm, plus
/// spare hand islands their reload/fire animations pose into view (e.g. the
/// pistol's off-hand holding a magazine). In VR the model renders in bind
/// pose from free viewpoints, so those spare hands float disembodied beside
/// the gun. Keep the largest arm island (the gripping hand and its sleeve)
/// and drop the rest; non-arm geometry is untouched.
pub struct VrHeldModel(pub Model);

fn process_vr_held_model(
    mesh: SystemShockContentModel,
    asset_cache: &mut AssetCache,
    _config: &(),
) -> VrHeldModel {
    let obj = match mesh {
        SystemShockContentModel::Obj(obj) => obj,
        other => return VrHeldModel(process_model(other, asset_cache, &())),
    };

    let islands = ss2_bin_obj_loader::connected_islands(&obj, is_first_person_arm_material);
    let drop: std::collections::HashSet<usize> =
        islands.into_iter().skip(1).flatten().collect();

    let obj = if drop.is_empty() {
        obj
    } else {
        let mut filtered = obj.clone();
        filtered.polygons = obj
            .polygons
            .iter()
            .enumerate()
            .filter(|(index, _)| !drop.contains(index))
            .map(|(_, polygon)| polygon.clone())
            .collect();
        filtered
    };

    VrHeldModel(Model::from_obj_bin(obj, asset_cache))
}

pub static VR_HELD_MODELS_IMPORTER: Lazy<AssetImporter<SystemShockContentModel, VrHeldModel, ()>> =
    Lazy::new(|| AssetImporter::define(load_model, process_vr_held_model));

/// Newtype so this importer gets its own [`AssetCache`] bucket.
///
/// The cache keys by `importer.type_id()`, which is the *type* of the importer,
/// so two importers that share `AssetImporter<_, Model, _>` would share one
/// bucket and silently serve whichever view of the model was requested first.
/// Any further view of an already-imported type needs its own newtype.
/// One authored hand, as its own model plus the frame it sits in.
pub struct FirstPersonHand {
    pub frame: ss2_bin_obj_loader::HandFrame,
    pub model: Model,
}

/// Every hand a first-person model draws, split apart. Newtype for cache
/// separation, as [`FirstPersonArm`].
pub struct FirstPersonHands(pub Vec<FirstPersonHand>);

fn process_first_person_hands(
    mesh: SystemShockContentModel,
    asset_cache: &mut AssetCache,
    _config: &(),
) -> FirstPersonHands {
    let SystemShockContentModel::Obj(obj) = mesh else {
        return FirstPersonHands(Vec::new());
    };

    FirstPersonHands(
        ss2_bin_obj_loader::split_connected(&obj, is_first_person_arm_material)
            .into_iter()
            .filter_map(|mut island| {
                let frame = ss2_bin_obj_loader::hand_frame(&island, is_first_person_arm_material)?;
                // Vhots belong to the weapon, and render as debug cubes.
                island.vhots.clear();

                Some(FirstPersonHand {
                    frame,
                    model: Model::from_obj_bin(island, asset_cache),
                })
            })
            .collect(),
    )
}

/// Each hand a first-person weapon model draws, as a separate model - the pose
/// library we snap between. Ordered largest-island first, so index 0 is the
/// main hand.
pub static FIRST_PERSON_HANDS_IMPORTER: Lazy<
    AssetImporter<SystemShockContentModel, FirstPersonHands, ()>,
> = Lazy::new(|| AssetImporter::define(load_model, process_first_person_hands));
