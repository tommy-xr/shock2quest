//! The hip holster: one weapon rides the dominant thigh, ready to draw.
//!
//! Unlike the belt card and the ammo pouch this one has *state* - which weapon
//! is in it - but only just: the slot stores the weapon's gamesys archetype in
//! `QuestInfo`, and the weapon itself stays in the pack as the same entity it
//! was, so its magazine and its condition are the live ones rather than a fresh
//! mint. Everything drawn here is then re-derived from that entity each frame,
//! the way the pouch derives its clip, so nothing about the picture needs
//! saving.

use cgmath::{Matrix4, Quaternion, Rotation3};

use crate::body_frame::BodyFrame;

/// The weapon in the holster this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HolsteredWeapon {
    /// Its gamesys archetype - the id the slot actually stores.
    pub template_id: i32,
    /// Its model, so the thigh shows the weapon the player will draw.
    pub model: String,
}

/// How a weapon with no authored holster pose hangs: a quarter turn about X
/// stands the model's long axis on end. The gun world models run barrel-first
/// along -Z with the grip at +Z, so -90 puts the muzzle at the floor and the
/// grip up - the way a weapon sits in a drop holster.
const DEFAULT_HOLSTERED_DEG: [f32; 3] = [-90.0, 0.0, 0.0];

/// How `model` is turned in the holster: its authored `holstered_deg`, else
/// [`DEFAULT_HOLSTERED_DEG`]. XYZ Euler degrees applied Z * Y * X, the same
/// convention the in-hand `rotation_deg` uses, so one authored number means the
/// same thing in both places.
pub fn holstered_rotation(model: &str) -> Quaternion<f32> {
    let [x, y, z] = crate::vr_grips::profile(model)
        .and_then(|profile| profile.holstered_deg)
        .unwrap_or(DEFAULT_HOLSTERED_DEG);
    Quaternion::from_angle_z(cgmath::Deg(z))
        * Quaternion::from_angle_y(cgmath::Deg(y))
        * Quaternion::from_angle_x(cgmath::Deg(x))
}

/// Whether `model` may be holstered at all. Everything may, unless its profile
/// says otherwise - the flag is there to retire a weapon that reads badly on
/// the thigh without touching code.
pub fn is_holsterable(model: &str) -> bool {
    crate::vr_grips::profile(model).is_none_or(|profile| profile.holster)
}

/// The weapon on the thigh: turned into its holstered pose, facing the way the
/// body does. Its size comes from the same `vr_grips` profile the hand uses, so
/// holster and hand can never disagree about how big a weapon is.
pub fn holster_transform(frame: &BodyFrame, model: &str) -> Matrix4<f32> {
    Matrix4::from_translation(frame.holster())
        * Matrix4::from(frame.rotation())
        * Matrix4::from(holstered_rotation(model))
        * Matrix4::from_scale(crate::vr_config::held_geometry_scale(model))
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
