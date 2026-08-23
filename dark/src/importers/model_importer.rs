use std::io::{Read, Seek};
use std::{path::PathBuf, rc::Rc};

use cgmath::{Matrix4, Point3, SquareMatrix, Vector3, Vector4};
use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use engine::scene::{
    MAX_SKINNED_JOINTS, SKINNING_PALETTE_SIZE, VertexPositionTextureSkinnedNormal,
};
use once_cell::sync::Lazy;

use crate::{
    ss2_bin_ai_loader::{self, SystemShock2AIMesh},
    ss2_bin_header,
    ss2_bin_obj_loader::{self, SystemShock2ObjectMesh},
    ss2_skeleton::Skeleton,
};

use crate::{model::Model, motion::AnimationPlayer};

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
/// The mesh renders exactly as authored, baked hand and forearm included. An
/// earlier revision stripped every arm island but the largest to hide the
/// spare hands some reload/fire animations pose into view - but "one arm
/// island = one hand" is false: the pistol (`atek_h`) splits its firing hand
/// (`ND-arm_atek.psd`) and sleeve (`ND-arm.psd`) into separate islands, so the
/// strip kept the sleeve and deleted the gripping hand. Spare-hand stripping
/// is deliberately dropped (a floating spare hand is the lesser evil); it can
/// return if a reliable classifier turns up.
pub struct VrHeldModel {
    pub model: Model,
    weapon_geometry: Option<VrHeldWeaponGeometry>,
}

#[derive(Clone)]
struct VrHeldWeaponGeometry {
    vertices: Vec<VertexPositionTextureSkinnedNormal>,
    /// The complement of `vertices` - the baked hand and forearm. Kept only so
    /// the arm can be *measured* against the weapon it holds: a first-person
    /// view model is authored for a flat camera's own projection, where an
    /// exaggerated weapon reads better, and VR draws it at true world scale.
    /// Whether a wield needs a uniform correction or a weapon-only one is
    /// exactly the ratio between these two.
    arm_vertices: Vec<VertexPositionTextureSkinnedNormal>,
    skeleton: Rc<Skeleton>,
    bind: Option<[Matrix4<f32>; MAX_SKINNED_JOINTS]>,
}

/// A cuboid fitted around the posed weapon-only geometry, expressed in the
/// held entity's local frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VrHeldWeaponBounds {
    pub size: Vector3<f32>,
    pub center: Vector3<f32>,
}

impl VrHeldModel {
    /// Fit a local AABB to the same skinned weapon vertices the renderer uses.
    /// `local_transform` is the model-space correction applied by the wield,
    /// so the result and rendered mesh stay in one coordinate system.
    pub fn posed_weapon_bounds(
        &self,
        player: &AnimationPlayer,
        local_transform: Matrix4<f32>,
    ) -> Option<VrHeldWeaponBounds> {
        let geometry = self.weapon_geometry.as_ref()?;
        let pose = player.get_transforms(&geometry.skeleton);
        let palette = match &geometry.bind {
            None => Skeleton::expand_skinning_palette(&pose, &geometry.skeleton),
            Some(bind) => {
                let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
                for joint in 0..MAX_SKINNED_JOINTS {
                    let posed = pose[joint] * bind[joint];
                    palette[joint] = posed;
                    palette[MAX_SKINNED_JOINTS + joint] = posed;
                }
                palette
            }
        };

        bounds_of_skinned_vertices(&geometry.vertices, &palette, local_transform)
    }

    /// The same fit over the baked hand/forearm instead of the weapon. Purely
    /// diagnostic: comparing it against [`Self::posed_weapon_bounds`] says
    /// whether a view model is uniformly oversized for VR or only its weapon
    /// is.
    pub fn posed_arm_bounds(
        &self,
        player: &AnimationPlayer,
        local_transform: Matrix4<f32>,
    ) -> Option<VrHeldWeaponBounds> {
        let geometry = self.weapon_geometry.as_ref()?;
        if geometry.arm_vertices.is_empty() {
            return None;
        }
        let pose = player.get_transforms(&geometry.skeleton);
        let palette = match &geometry.bind {
            None => Skeleton::expand_skinning_palette(&pose, &geometry.skeleton),
            Some(bind) => {
                let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
                for joint in 0..MAX_SKINNED_JOINTS {
                    let posed = pose[joint] * bind[joint];
                    palette[joint] = posed;
                    palette[MAX_SKINNED_JOINTS + joint] = posed;
                }
                palette
            }
        };
        bounds_of_skinned_vertices(&geometry.arm_vertices, &palette, local_transform)
    }
}

fn is_melee_arm_material(name: &str) -> bool {
    name.to_ascii_lowercase().contains("melee_arm")
}

/// Split geometry into (terminal-joint weapon, everything else). The weapon is
/// the rigid geometry on the last rendered joint; the remainder is the arm.
fn split_at_terminal_joint(
    vertices: impl IntoIterator<Item = VertexPositionTextureSkinnedNormal>,
) -> (
    Vec<VertexPositionTextureSkinnedNormal>,
    Vec<VertexPositionTextureSkinnedNormal>,
) {
    let all = vertices.into_iter().collect::<Vec<_>>();
    let rigid_joint = |vertex: &VertexPositionTextureSkinnedNormal| {
        let mut bindings = vertex
            .bone_indices
            .into_iter()
            .zip(vertex.bone_weights)
            .filter(|(_, weight)| *weight > 0.0);
        let (joint, _) = bindings.next()?;
        (joint < MAX_SKINNED_JOINTS as u32 && bindings.next().is_none()).then_some(joint)
    };
    let Some(terminal_joint) = all.iter().filter_map(rigid_joint).max() else {
        return (Vec::new(), all);
    };
    all.into_iter()
        .partition(|vertex| rigid_joint(vertex) == Some(terminal_joint))
}

fn weapon_geometry(mesh: &SystemShockContentModel) -> Option<VrHeldWeaponGeometry> {
    let SystemShockContentModel::Mesh(ai_mesh, skeleton, pmnm) = mesh else {
        return None;
    };

    if let Some(pmnm) = pmnm {
        let runs = pmnm.to_skinned_vertices();
        let has_named_arm = runs
            .iter()
            .any(|(material, _)| is_melee_arm_material(material));
        let (vertices, arm_vertices) = if has_named_arm {
            let (arm, weapon): (Vec<_>, Vec<_>) = runs
                .into_iter()
                .partition(|(material, _)| is_melee_arm_material(material));
            let flatten = |runs: Vec<(String, Vec<VertexPositionTextureSkinnedNormal>)>| {
                runs.into_iter()
                    .flat_map(|(_, vertices)| vertices)
                    .collect::<Vec<_>>()
            };
            (flatten(weapon), flatten(arm))
        } else {
            split_at_terminal_joint(runs.into_iter().flat_map(|(_, vertices)| vertices))
        };
        if vertices.is_empty() {
            return None;
        }
        return Some(VrHeldWeaponGeometry {
            vertices,
            arm_vertices,
            skeleton: skeleton.clone(),
            bind: Some(ss2_bin_ai_loader::pmnm_bind_matrices(skeleton)),
        });
    }

    // The classic LGMM melee rigs combine hand and weapon under one material.
    // Their weapon is the rigid geometry on the terminal rendered joint; this
    // data-derived fallback includes the fist but excludes the forearm instead
    // of assuming a model name or authored dimension.
    let (vertices, arm_vertices) = split_at_terminal_joint(
        ss2_bin_ai_loader::to_vertices(ai_mesh, skeleton)
            .0
            .into_iter()
            .flat_map(|(_, vertices)| vertices),
    );
    (!vertices.is_empty()).then_some(VrHeldWeaponGeometry {
        vertices,
        arm_vertices,
        skeleton: skeleton.clone(),
        bind: None,
    })
}

fn bounds_of_skinned_vertices(
    vertices: &[VertexPositionTextureSkinnedNormal],
    palette: &[Matrix4<f32>; SKINNING_PALETTE_SIZE],
    local_transform: Matrix4<f32>,
) -> Option<VrHeldWeaponBounds> {
    let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);

    for vertex in vertices {
        let point = Vector4::new(vertex.position.x, vertex.position.y, vertex.position.z, 1.0);
        let mut skinned = Vector4::new(0.0, 0.0, 0.0, 0.0);
        let mut total_weight = 0.0;
        for (joint, weight) in vertex.bone_indices.iter().zip(vertex.bone_weights) {
            if weight > 0.0 {
                let matrix = palette.get(*joint as usize)?;
                skinned += matrix * point * weight;
                total_weight += weight;
            }
        }
        let skinned = if total_weight > 0.0 { skinned } else { point };
        let local = local_transform * skinned;
        let p = Point3::new(local.x, local.y, local.z);
        if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
            return None;
        }
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }

    (min.x.is_finite() && max.x.is_finite()).then_some(VrHeldWeaponBounds {
        size: max - min,
        center: (min + max) * 0.5,
    })
}

fn process_vr_held_model(
    mesh: SystemShockContentModel,
    asset_cache: &mut AssetCache,
    _config: &(),
) -> VrHeldModel {
    let weapon_geometry = weapon_geometry(&mesh);
    VrHeldModel {
        model: process_model(mesh, asset_cache, &()),
        weapon_geometry,
    }
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

#[cfg(test)]
mod tests {
    use cgmath::{Matrix4, SquareMatrix, vec2, vec3};

    use super::*;

    fn vertex(
        position: Vector3<f32>,
        bone_indices: [u32; 4],
        bone_weights: [f32; 4],
    ) -> VertexPositionTextureSkinnedNormal {
        VertexPositionTextureSkinnedNormal {
            position,
            uv: vec2(0.0, 0.0),
            normal: vec3(0.0, 0.0, 1.0),
            bone_indices,
            bone_weights,
        }
    }

    #[test]
    fn classic_weapon_fallback_keeps_only_the_terminal_rigid_joint() {
        let hand = vertex(vec3(0.0, 0.0, 0.0), [2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let weapon = vertex(vec3(0.0, 1.0, 0.0), [3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let stretchy = vertex(vec3(0.0, 0.5, 0.0), [3, 43, 0, 0], [0.5, 0.5, 0.0, 0.0]);

        let (fitted, arm) =
            split_at_terminal_joint([hand.clone(), weapon.clone(), stretchy.clone()]);

        assert_eq!(fitted.len(), 1);
        assert_eq!(fitted[0].position, weapon.position);
        // The remainder is the arm side: everything the weapon fit excludes,
        // stretchy multi-joint vertices included.
        assert_eq!(arm.len(), 2);
        assert!(arm.iter().any(|v| v.position == hand.position));
        assert!(arm.iter().any(|v| v.position == stretchy.position));
    }

    #[test]
    fn posed_bounds_apply_skinning_and_the_renderers_local_correction() {
        let vertices = [
            vertex(vec3(-1.0, 0.0, -0.5), [0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
            vertex(vec3(1.0, 2.0, 0.5), [0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ];
        let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
        palette[0] = Matrix4::from_translation(vec3(0.0, 3.0, 0.0));
        let correction = Matrix4::from_translation(vec3(2.0, -1.0, 4.0));

        let bounds = bounds_of_skinned_vertices(&vertices, &palette, correction).unwrap();

        assert_eq!(bounds.size, vec3(2.0, 2.0, 1.0));
        assert_eq!(bounds.center, vec3(2.0, 3.0, 4.0));
    }

    #[test]
    fn anniversary_melee_arm_material_is_not_weapon_geometry() {
        assert!(is_melee_arm_material("ND-melee_arm.psd"));
        assert!(!is_melee_arm_material("ND-wrench.psd"));
    }
}
