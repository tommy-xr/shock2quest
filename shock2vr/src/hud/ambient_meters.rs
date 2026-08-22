//! Prototype "ambient meters" (experimental flag [`FEATURE`]): ammo lives on
//! the wielded weapon, health stays on the left wrist but only shows when the
//! wrist is glanced at (rotated toward the face).
//!
//! Layout/placement decisions live HERE (and in [`super::ammo_panel`], which
//! authors the readout's pixels) - a presentation supplies only a root
//! transform (AGENTS.md section 3). Only the VR presentation consumes these
//! today; the flat HUD is untouched by the flag.

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Get, View, World};

use crate::runtime_props::RuntimePropTransform;
use crate::vr_config::Handedness;

use super::ammo_panel;
use super::virtual_arms;

/// The experimental-features name gating all of this.
pub(crate) const FEATURE: &str = "ambient_meters";

// --- On-weapon ammo meter placement (weapon-local, tuning knobs) -------------
//
// Weapon-local frame, for the 25AE `_h` view models VR wields: barrel along
// -X, +X back toward the shooter, +Y up out of the gun body (see
// `vr_config`'s "authored barrel along -X" note). Offsets are in world units
// (1 unit = 0.762 m), the same space `RuntimePropTransform` lives in.

/// Where the meter hangs off the weapon origin (the grip): above and slightly
/// behind the gun body.
pub(crate) const WEAPON_METER_OFFSET: Vector3<f32> = vec3(0.05, 0.17, 0.0);
/// Lean the panel top away from the shooter so it reads like a rear sight
/// display when the gun is held in a natural aim pose.
pub(crate) const WEAPON_METER_TILT: Deg<f32> = Deg(-20.0);
/// Panel size in world units, preserving the AMMOFULL 260x64 aspect.
pub(crate) const WEAPON_METER_WIDTH: f32 = 0.20;
pub(crate) const WEAPON_METER_HEIGHT: f32 = WEAPON_METER_WIDTH * (64.0 / 260.0);

/// Root transform for the on-weapon ammo panel: canvas convention (+Z at the
/// viewer, +X the viewer's right) hung in the weapon's local frame, facing
/// back along the barrel toward the shooter. The single placement decision -
/// any presentation that draws this meter maps the same canvas through it.
pub(crate) fn weapon_meter_transform(weapon_transform: Matrix4<f32>) -> Matrix4<f32> {
    weapon_transform
        * Matrix4::from_translation(WEAPON_METER_OFFSET)
        // Panel +Z -> weapon +X (toward the shooter); panel +X -> weapon -Z,
        // which is the viewer's right when looking down +X at the panel.
        * Matrix4::from_angle_y(Deg(90.0))
        * Matrix4::from_angle_x(WEAPON_METER_TILT)
        * Matrix4::from_nonuniform_scale(WEAPON_METER_WIDTH, WEAPON_METER_HEIGHT, 1.0)
}

/// Spacing between the backdrop and the readout canvas, matching the forearm
/// panels' overlay step.
const OVERLAY_Z_OFFSET: f32 = 0.001;

/// The on-weapon ammo meter: the same AMMOFULL backdrop + shared
/// [`ammo_panel`] readout the right forearm wears today, hung off the wielded
/// weapon's live `RuntimePropTransform` instead (the anchor
/// `systems/attachment.rs` uses for muzzle flashes). Empty when nothing with
/// a clip (or the psi amp) is wielded, or the weapon has no transform.
pub(crate) fn create_weapon_ammo_meter(
    asset_cache: &mut AssetCache,
    world: &World,
) -> Vec<SceneObject> {
    let Some(weapon) = crate::wielded_weapon::wielded_weapon(world) else {
        return Vec::new();
    };
    let readout = ammo_panel::AmmoReadout::from_world(world, false);
    if readout.is_empty() {
        return Vec::new();
    }
    let Ok(weapon_transform) = world
        .borrow::<View<RuntimePropTransform>>()
        .map(|v| v.get(weapon).map(|t| t.0))
    else {
        return Vec::new();
    };
    let Ok(weapon_transform) = weapon_transform else {
        return Vec::new();
    };

    let root = weapon_meter_transform(weapon_transform);
    let mut objects = vec![virtual_arms::panel_backdrop(
        asset_cache,
        "AMMOFULL.PCX",
        root,
    )];
    objects.append(
        &mut ammo_panel::build_readout_canvas(&readout).render_world_space(
            asset_cache,
            root * Matrix4::from_translation(vec3(0.0, 0.0, OVERLAY_Z_OFFSET)),
            None,
            None,
            OVERLAY_Z_OFFSET,
        ),
    );
    objects
}

// --- Glance-gated wrist health ----------------------------------------------

/// Show the wrist panel when its outward normal points this well toward the
/// eye...
pub(crate) const GLANCE_SHOW_DOT: f32 = 0.55;
/// ...and keep it until alignment drops below this (hysteresis so the panel
/// does not flicker at the boundary).
pub(crate) const GLANCE_HIDE_DOT: f32 = 0.30;

/// Poses arrive as the ZERO quaternion while untracked, and cgmath's
/// `rotate_vector` silently passes the input through - treat near-zero as
/// "untracked", never as a valid pose (vr-ui-design rule 7).
pub(crate) fn is_tracked(rotation: Quaternion<f32>) -> bool {
    rotation.magnitude2() > 1e-6
}

/// How well the left wrist panel faces the eye this frame: the dot of the
/// panel's outward normal (local +Z of the shared forearm pose - the same
/// side its overlays stack toward) with the unit vector from the panel to the
/// eye. `None` when the hand pose is untracked or the eye is on the panel.
pub(crate) fn wrist_glance_alignment(
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    eye_position: Vector3<f32>,
) -> Option<f32> {
    if !is_tracked(left_hand_rotation) {
        return None;
    }
    let (panel_position, panel_rotation) =
        virtual_arms::forearm_pose(left_hand_position, left_hand_rotation, Handedness::Left);
    let normal = panel_rotation.rotate_vector(vec3(0.0, 0.0, 1.0));
    let to_eye = eye_position - panel_position;
    if to_eye.magnitude2() < 1e-6 {
        return None;
    }
    Some(normal.dot(to_eye.normalize()))
}

/// Hysteresis latch for the wrist-glance gate.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct GlanceState {
    visible: bool,
}

impl GlanceState {
    /// Feed one frame's alignment: the dot of the panel's outward normal with
    /// the unit vector from the panel toward the eye. `None` means the pose
    /// was untracked this frame - the panel hides rather than latching a
    /// stale/garbage pose.
    pub(crate) fn update(&mut self, alignment: Option<f32>) -> bool {
        self.visible = match alignment {
            None => false,
            Some(dot) if self.visible => dot > GLANCE_HIDE_DOT,
            Some(dot) => dot > GLANCE_SHOW_DOT,
        };
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::SquareMatrix;

    #[test]
    fn glance_gate_uses_hysteresis_not_a_single_threshold() {
        let mut gate = GlanceState::default();
        // Below the show threshold: stays hidden.
        assert!(!gate.update(Some(0.5)));
        // Crosses the show threshold: appears.
        assert!(gate.update(Some(0.6)));
        // Dips between the two thresholds: STAYS visible (no flicker).
        assert!(gate.update(Some(0.4)));
        // Drops below the hide threshold: disappears.
        assert!(!gate.update(Some(0.2)));
        // Back in the dead band while hidden: STAYS hidden.
        assert!(!gate.update(Some(0.4)));
    }

    #[test]
    fn an_untracked_pose_hides_the_panel_and_clears_the_latch() {
        let mut gate = GlanceState::default();
        assert!(gate.update(Some(0.9)));
        assert!(!gate.update(None), "untracked frame must hide, not hold");
        // After tracking resumes mid-deadband, the gate re-arms from hidden.
        assert!(!gate.update(Some(0.4)));
    }

    #[test]
    fn the_zero_quaternion_is_untracked() {
        assert!(!is_tracked(Quaternion::new(0.0, 0.0, 0.0, 0.0)));
        assert!(is_tracked(Quaternion::new(1.0, 0.0, 0.0, 0.0)));
    }

    #[test]
    fn the_weapon_meter_hangs_at_its_authored_offset() {
        let root = weapon_meter_transform(Matrix4::identity());
        // The translation column is exactly the tuning offset (rotation/scale
        // do not move the panel origin).
        assert_eq!(
            vec3(root.w.x, root.w.y, root.w.z),
            WEAPON_METER_OFFSET,
            "panel centre must sit at the weapon-local tuning offset"
        );
    }

    #[test]
    fn the_weapon_meter_faces_back_toward_the_shooter() {
        // With no tilt the panel's +Z (its outward face) must map to the
        // weapon's +X - the direction back along the barrel at the shooter.
        let root = weapon_meter_transform(Matrix4::identity());
        let face = root * cgmath::vec4(0.0, 0.0, 1.0, 0.0);
        assert!(
            face.x > 0.9,
            "panel normal should point at the shooter (+X), got {face:?}"
        );
    }
}
