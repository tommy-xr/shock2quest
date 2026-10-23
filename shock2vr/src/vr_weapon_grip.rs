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
pub fn model_frame(
    source: &dark::importers::GloveWeaponModel,
    name: &str,
    hand: Handedness,
) -> Matrix4<f32> {
    if let Some(arm) = source
        .melee_joints
        .as_ref()
        .and_then(|joints| vr_config::MeleePosedArm::from_joints(joints))
    {
        melee_frame(arm, hand)
    } else {
        vr_config::gun_model_frame(name, hand)
    }
}

fn melee_frame(arm: vr_config::MeleePosedArm, hand: Handedness) -> Matrix4<f32> {
    Matrix4::from_translation(vr_config::melee_contact_offset_scaled(arm, 1.0))
        * vr_config::melee_wield_pose_correction_scaled(arm, 1.0, hand)
}

/// Reflection between the actual rendered weapon frames, shared by support
/// gameplay and authoring. Asset arm data decides the frame, not a name list.
pub fn model_mirror(cache: &mut AssetCache, name: &str) -> Option<Matrix4<f32>> {
    let source = cache.get_opt(&GLOVE_WEAPON_IMPORTER, name)?;
    let source = source.as_ref().as_ref()?;
    let right = model_frame(source, name, Handedness::Right);
    let left = model_frame(source, name, Handedness::Left);
    Some(left * right.invert()?)
}

/// `model_mirror` from the name alone, for tools without a GL context to load
/// the model: a gun reflects across its barrel plane, a melee rig across the
/// hand's (its contact point lies on that plane). `None` for anything drawn
/// alike in either hand.
pub fn named_model_mirror(name: &str) -> Option<Matrix4<f32>> {
    if !supports_model(name) {
        return None;
    }
    if is_melee(name) {
        return Some(Handedness::Left.mirror());
    }
    let frame = |hand| vr_config::gun_model_frame(name, hand);
    Some(frame(Handedness::Left) * frame(Handedness::Right).invert()?)
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
    let mirror = model_frame(source, name, hand);
    let triangles: Vec<_> = source
        .triangles
        .iter()
        .map(|t| {
            let t = t.map(|p| mirror.transform_point(p));
            if mirror.determinant() < 0.0 {
                [t[0], t[2], t[1]]
            } else {
                t
            }
        })
        .collect();
    // The authored arm remains a handed fit guide. A gun-only reflection
    // must not turn that guide (or the replacement glove) into the other hand.
    let arm_frame = if source.melee_joints.is_some() {
        mirror
    } else {
        hand.gun_mirror()
    };
    let arms: Vec<_> = source
        .arm_triangles
        .iter()
        .flatten()
        .map(|p| arm_frame.transform_point(*p))
        .collect();
    let mut fingerprint_geometry = triangles.clone();
    fingerprint_geometry.extend(arms.chunks_exact(3).map(|p| [p[0], p[1], p[2]]));
    Some((
        triangles,
        arms,
        crate::vr_grip::surface_fingerprint(&fingerprint_geometry),
    ))
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
            grip = grip.with_item_scale(scale);
            if is_melee(name) {
                grip.pose_family = "cylindrical".to_owned();
            }
            if best.as_ref().is_none_or(|b| grip.score > b.score) {
                best = Some(grip);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    /// merge-grips mirrors melee by name; play mirrors by the asset's posed
    /// arm. They agree for any arm: the contact point lies on the hand's
    /// mirror plane by construction.
    #[test]
    fn the_named_melee_mirror_is_the_posed_arm_mirror_for_any_arm() {
        let arms = [
            ([0.0, -0.3, 0.1], [0.0, 0.0, 0.0], [0.02, 0.4, -0.05]),
            ([0.3, 0.1, -0.2], [0.05, 0.02, 0.01], [-0.2, 0.3, 0.25]),
            ([-0.1, -0.25, 0.35], [0.1, -0.1, 0.2], [0.4, 0.05, -0.1]),
        ]
        .map(|(elbow, fist, weapon)| vr_config::MeleePosedArm {
            elbow: elbow.into(),
            fist: fist.into(),
            weapon: weapon.into(),
        });
        for name in ["wrench_h", "rapier_h", "shard_h", "psword_h"] {
            let named = named_model_mirror(name).unwrap();
            for arm in arms {
                let posed = melee_frame(arm, Handedness::Left)
                    * melee_frame(arm, Handedness::Right).invert().unwrap();
                for (a, b) in [posed.x, posed.y, posed.z, posed.w]
                    .iter()
                    .zip([named.x, named.y, named.z, named.w])
                {
                    assert!((a - b).magnitude() < 1e-4, "{name} {arm:?}: {posed:?}");
                }
            }
        }
    }
}
