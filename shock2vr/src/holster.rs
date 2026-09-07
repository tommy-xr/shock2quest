//! The hip holster: one weapon rides the dominant thigh, ready to draw.
//!
//! Unlike the belt card and the ammo pouch this one has *state* - which weapon
//! is in it - but only just: the slot stores the weapon's gamesys archetype in
//! `QuestInfo`, and the weapon itself stays in the pack as the same entity it
//! was, so its magazine and its condition are the live ones rather than a fresh
//! mint. Everything drawn here is then re-derived from that entity each frame,
//! the way the pouch derives its clip, so nothing about the picture needs
//! saving.

use cgmath::{Matrix4, Quaternion};

use crate::body_frame::BodyFrame;

/// The weapon in the holster this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HolsteredWeapon {
    /// Its model, so the thigh shows the weapon the player will draw.
    pub model: String,
}

/// How a weapon with no authored holster pose hangs: a quarter turn about X
/// stands the model's long axis on end, muzzle down and grip up, the way a
/// weapon sits in a drop holster.
///
/// It is a **placeholder, not a rule**. The world models do not share an axis
/// convention - the pistol and the assault rifle run barrel-first along -Z, so
/// -90 turns them the right way up, while the shotgun runs the other way and
/// authors `holstered_deg` to say so. A model nobody has looked at hangs
/// whichever way its own axes fall; authoring it is a `holstered_deg` entry.
const DEFAULT_HOLSTERED_DEG: [f32; 3] = [-90.0, 0.0, 0.0];

/// How `model` is turned in the holster: its authored `holstered_deg`, else
/// [`DEFAULT_HOLSTERED_DEG`]. XYZ Euler degrees applied Z * Y * X, the same
/// convention the in-hand `rotation_deg` uses, so one authored number means the
/// same thing in both places.
pub fn holstered_rotation(model: &str) -> Quaternion<f32> {
    crate::vr_grips::euler_zyx_deg(
        crate::vr_grips::profile(model)
            .and_then(|profile| profile.holstered_deg)
            .unwrap_or(DEFAULT_HOLSTERED_DEG),
    )
}

/// Whether `model` may be holstered at all. Everything may, unless its profile
/// says otherwise - the flag is there to retire a weapon that reads badly on
/// the thigh without touching code.
pub fn is_holsterable(model: &str) -> bool {
    crate::vr_grips::profile(model).is_none_or(|profile| profile.holster)
}

/// The weapon on the thigh: turned into its holstered pose, facing the way the
/// body does, at its authored holstered size.
pub fn holster_transform(frame: &BodyFrame, model: &str) -> Matrix4<f32> {
    crate::body_frame::worn_transform(
        frame,
        frame.holster(),
        holstered_scale(model),
        holstered_rotation(model),
    )
}

/// How big `model` is drawn on the thigh: its authored `holstered_scale`, else
/// the size a hand would hold it at.
///
/// A separate number from the in-hand `scale` because the two paths measure
/// different geometry. VR wields a gun's shrunk-to-life-size `_h` view model,
/// which reports 1.0 here, so the holster - which wears the *world* model, at
/// its full authored size - needs its own life-size figure. Making the world
/// model's own `scale` carry it would silently resize every held world model on
/// a classic install, which is not this anchor's business.
pub fn holstered_scale(model: &str) -> f32 {
    crate::vr_grips::profile(model)
        .and_then(|profile| profile.holstered_scale)
        .unwrap_or_else(|| crate::vr_config::held_geometry_scale(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Rotation, vec3};

    /// With nothing authored the muzzle points at the floor: the model's own
    /// -Z (the barrel end of every gun world model) ends up pointing down, and
    /// the grip at +Z points up.
    #[test]
    fn an_unprofiled_weapon_hangs_barrel_down() {
        let _guard = crate::vr_grips::test_guard();
        crate::vr_grips::set_profiles(Default::default());

        let barrel = holstered_rotation("nothing_authored").rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!(
            barrel.y < -0.99,
            "the barrel should point at the floor, got {barrel:?}"
        );
    }

    /// An authored pose replaces the default outright.
    #[test]
    fn an_authored_pose_wins() {
        let _guard = crate::vr_grips::test_guard();
        crate::vr_grips::set_profiles(
            crate::vr_grips::parse(r#"{ "atek": { "holstered_deg": [0.0, 0.0, 0.0] } }"#).unwrap(),
        );

        let along = holstered_rotation("atek").rotate_vector(vec3(0.0, 0.0, 1.0));
        assert!(
            (along - vec3(0.0, 0.0, 1.0)).magnitude() < 1e-5,
            "an authored zero turn should leave the model as authored, got {along:?}"
        );
    }

    /// The holster wears the world model at its own authored size, and falls
    /// back to the in-hand size only where none is authored - a gun's `_h`
    /// scale must not stand in for the world model's, which is a different
    /// mesh at a different size.
    #[test]
    fn a_holstered_size_is_its_own_number() {
        let _guard = crate::vr_grips::test_guard();
        crate::vr_grips::set_profiles(
            crate::vr_grips::parse(
                r#"{ "atek_w": { "scale": 0.9, "holstered_scale": 0.44 },
                     "mug": { "scale": 0.5 } }"#,
            )
            .unwrap(),
        );

        assert!((holstered_scale("atek_w") - 0.44).abs() < 1e-6);
        assert!(
            (holstered_scale("mug") - 0.5).abs() < 1e-6,
            "with nothing authored the holster falls back to the held size"
        );
    }

    /// A profile can retire a model from the holster; anything unprofiled is
    /// allowed, so a data set that says nothing holsters everything.
    #[test]
    fn a_profile_can_refuse_the_holster() {
        let _guard = crate::vr_grips::test_guard();
        crate::vr_grips::set_profiles(
            crate::vr_grips::parse(r#"{ "fsn_h": { "holster": false } }"#).unwrap(),
        );

        assert!(!is_holsterable("fsn_h"));
        assert!(is_holsterable("atek"));
    }
}
