//! Authored weapon hands guide offline fitting; only weapon geometry is drawn.
use cgmath::{Matrix4, One, Point3, Quaternion, SquareMatrix, Transform, Vector3, Zero};
use dark::importers::GLOVE_WEAPON_IMPORTER;
use engine::assets::asset_cache::AssetCache;

use crate::{
    Handedness, vr_config,
    vr_grip::{GripKinematics, GripSurface, ResolvedGrip},
};

pub fn supports_model(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().trim_end_matches(".bin"),
        "atek_h"
            | "ar15_h"
            | "sg_h"
            | "lasehand"
            | "empgun_h"
            | "gren_h"
            | "sfg_h"
            | "fsn_h"
            | "al_h"
            | "viro_h"
            | "wrench_h"
            | "rapier_h"
            | "shard_h"
            | "psword_h"
    )
}

pub fn is_melee(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().trim_end_matches(".bin"),
        "wrench_h" | "rapier_h" | "shard_h" | "psword_h"
    )
}

/// Guns stay in their authored barrel frame. Melee uses the posed fist/weapon
/// joints, matching the gameplay correction before its contact-origin split.
pub fn model_frame(source: &dark::importers::GloveWeaponModel, hand: Handedness) -> Matrix4<f32> {
    if let Some(arm) = source
        .melee_joints
        .as_ref()
        .and_then(|joints| vr_config::MeleePosedArm::from_joints(joints))
    {
        Matrix4::from_translation(vr_config::melee_contact_offset_scaled(arm, 1.0))
            * vr_config::melee_wield_pose_correction_scaled(arm, 1.0, hand)
    } else {
        hand.gun_mirror()
    }
}

/// Reflection between the actual rendered weapon frames, shared by support
/// gameplay and authoring. Asset arm data decides the frame, not a name list.
pub fn model_mirror(cache: &mut AssetCache, name: &str) -> Option<Matrix4<f32>> {
    let source = cache.get_opt(&GLOVE_WEAPON_IMPORTER, name)?;
    let source = source.as_ref().as_ref()?;
    let right = model_frame(source, Handedness::Right);
    let left = model_frame(source, Handedness::Left);
    Some(left * right.invert()?)
}

/// Geometry is reflected exactly as the rendered gun. The canonical hash also
/// includes the authored arms: changing the fit guide invalidates its bake.
pub fn inputs(
    cache: &mut AssetCache,
    name: &str,
    hand: Handedness,
) -> Option<(Vec<[Point3<f32>; 3]>, Vec<Point3<f32>>, String)> {
    let source = cache.get_opt(&GLOVE_WEAPON_IMPORTER, name)?;
    let source = source.as_ref().as_ref()?;
    let mirror = model_frame(source, hand);
    let triangles: Vec<_> = source
        .triangles
        .iter()
        .map(|t| {
            let t = t.map(|p| mirror.transform_point(p));
            if hand == Handedness::Left {
                [t[0], t[2], t[1]]
            } else {
                t
            }
        })
        .collect();
    let arms: Vec<_> = source
        .arm_triangles
        .iter()
        .flatten()
        .map(|p| mirror.transform_point(*p))
        .collect();
    let fingerprint = if source.melee_joints.is_some() {
        // Hash the inputs to posing, not its floating-point output: the same
        // wrench on Quest and desktop can straddle a quantization boundary.
        use std::io::Read;
        let skeleton_name = std::path::Path::new(name).with_extension("cal");
        let files = [
            name,
            skeleton_name.to_str()?,
            dark::importers::GLOVE_MELEE_POSE_CLIP,
            "motiondb.bin",
        ];
        let mut sources = Vec::new();
        for file in files {
            let mut bytes = Vec::new();
            cache
                .get_raw_reader(file)?
                .borrow_mut()
                .read_to_end(&mut bytes)
                .ok()?;
            sources.push(bytes);
        }
        melee_source_fingerprint([&sources[0], &sources[1], &sources[2], &sources[3]], hand)
    } else {
        let mut fingerprint_geometry = triangles.clone();
        fingerprint_geometry.extend(arms.chunks_exact(3).map(|p| [p[0], p[1], p[2]]));
        crate::vr_grip::surface_fingerprint(&fingerprint_geometry)
    };
    Some((triangles, arms, fingerprint))
}

pub fn resolve(
    name: &str,
    hand: Handedness,
    triangles: &[[Point3<f32>; 3]],
    arms: &[Point3<f32>],
    rig: &GripKinematics,
) -> Option<ResolvedGrip> {
    let seed = if is_melee(name) {
        vr_config::VRHandModelPerHandAdjustments {
            offset: Vector3::zero(),
            rotation: Quaternion::one(),
        }
    } else {
        vr_config::get_vr_hand_model_adjustments_from_model(name.trim_end_matches(".bin"), hand)
    };
    // Some remaster heavy weapons have no authored hands. In that case the
    // legacy grip bounds the surface search; it is a weaker placement guide.
    let fallback: Vec<_> = if arms.is_empty() {
        triangles.iter().flatten().copied().collect()
    } else {
        Vec::new()
    };
    let arms = if arms.is_empty() { &fallback } else { arms };
    let mut best: Option<ResolvedGrip> = None;
    for scale in if is_melee(name) {
        [0.7, 0.85, 1.0]
    } else {
        [0.4, 0.55, 0.7]
    } {
        let transform = Matrix4::from_scale(scale);
        let scaled: Vec<_> = triangles
            .iter()
            .map(|t| t.map(|p| transform.transform_point(p)))
            .collect();
        let arms: Vec<_> = arms.iter().map(|p| transform.transform_point(*p)).collect();
        let surface = GripSurface::new(&scaled)?;
        if let Some(mut grip) =
            surface.resolve_weapon(rig, &arms, seed.offset * scale, seed.rotation)
        {
            grip.item_scale = scale;
            if is_melee(name) {
                grip.pose_family = "cylindrical".to_owned();
            }
            grip.anchor = grip.anchor.map(|v| v / scale);
            grip.contacts = grip.contacts.map(|c| c.map(|p| p.map(|v| v / scale)));
            if best.as_ref().is_none_or(|b| grip.score > b.score) {
                best = Some(grip);
            }
        }
    }
    best
}

/// Version the PMNM skinning/model-frame recipe here when it changes. File
/// lengths delimit dependencies, so moving bytes between files changes the key.
fn melee_source_fingerprint(sources: [&[u8]; 4], hand: Handedness) -> String {
    let version = b"melee-pose-v1";
    let hand = if hand == Handedness::Left { 0 } else { 1 };
    let bytes = version
        .iter()
        .copied()
        .chain([hand])
        .chain(sources.into_iter().flat_map(|bytes| {
            (bytes.len() as u64)
                .to_le_bytes()
                .into_iter()
                .chain(bytes.iter().copied())
        }));
    format!(
        "melee-source-v1:{}",
        crate::vr_grip::fingerprint_bytes(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melee_source_key_tracks_each_dependency_and_hand_without_posed_floats() {
        let source: [&[u8]; 4] = [b"model", b"skeleton", b"animation", b"motiondb"];
        let key = melee_source_fingerprint(source, Handedness::Right);
        assert_eq!(key, melee_source_fingerprint(source, Handedness::Right));
        assert_ne!(key, melee_source_fingerprint(source, Handedness::Left));
        for i in 0..4 {
            let mut changed = source;
            changed[i] = b"changed";
            assert_ne!(key, melee_source_fingerprint(changed, Handedness::Right));
        }
        assert_ne!(
            melee_source_fingerprint([b"ab", b"c", b"d", b"e"], Handedness::Right),
            melee_source_fingerprint([b"a", b"bc", b"d", b"e"], Handedness::Right),
        );
    }
}
