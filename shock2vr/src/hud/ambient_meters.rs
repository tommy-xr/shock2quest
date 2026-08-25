//! Prototype "ambient meters" (experimental flag [`FEATURE`]): ammo lives on
//! the wielded weapon, health stays on the left wrist but only shows when the
//! wrist is glanced at (rotated toward the face), and the psi amp carries its
//! own pool/overload meter.
//!
//! Layout/placement decisions live HERE (and in [`super::ammo_panel`] /
//! [`super::psi_amp_panel`], which author the readouts' pixels) - a
//! presentation supplies only a root transform (AGENTS.md section 3). The
//! on-weapon ammo tag and glance-gated wrist health are VR-only (the flat HUD
//! is untouched by the flag for those); the on-amp psi meter draws in BOTH
//! presentations - see [`amp_psi_meter_root`] and `mission_core`'s flat
//! viewmodel draw.

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Get, View, World};

use crate::runtime_props::RuntimePropTransform;
use crate::vr_config::Handedness;

use super::ammo_panel;
use super::psi_amp_panel;
use super::virtual_arms;
use virtual_arms::OVERLAY_Z_OFFSET;

/// The experimental-features name gating all of this.
pub(crate) const FEATURE: &str = "ambient_meters";

// --- On-weapon ammo meter placement (weapon-local, tuning knobs) -------------
//
// Weapon-local frame, for the 25AE `_h` view models VR wields: barrel along
// -X, +X back toward the shooter, +Y up out of the gun body (see
// `vr_config`'s "authored barrel along -X" note). Offsets are in world units
// (1 unit = 0.762 m), the same space `RuntimePropTransform` lives in.
// The per-weapon anchor itself lives in `vr_config::weapon_meter_anchor_*`,
// beside the grip-offset table; these are the shared shape knobs.

/// Lean the panel top away from the shooter so it reads like a rear-sight
/// status tag when the gun is held in a natural aim pose.
pub(crate) const WEAPON_METER_TILT: Deg<f32> = Deg(-20.0);
/// Panel size in world units, preserving the compact AMMOBACK crop's 94x64
/// aspect.
pub(crate) const WEAPON_METER_WIDTH: f32 = 0.10;
pub(crate) const WEAPON_METER_HEIGHT: f32 =
    WEAPON_METER_WIDTH * (ammo_panel::COMPACT_H / ammo_panel::COMPACT_W);

/// Root transform for the on-weapon ammo panel: canvas convention (+Z at the
/// viewer, +X the viewer's right) hung at `anchor` in the weapon's local
/// frame, facing back along the barrel toward the shooter. The single
/// placement decision - any presentation that draws this meter maps the same
/// canvas through it.
pub(crate) fn weapon_meter_transform(
    weapon_transform: Matrix4<f32>,
    anchor: Vector3<f32>,
) -> Matrix4<f32> {
    weapon_transform
        * Matrix4::from_translation(anchor)
        // Panel +Z -> weapon +X (toward the shooter); panel +X -> weapon -Z,
        // which is the viewer's right when looking down +X at the panel.
        * Matrix4::from_angle_y(Deg(90.0))
        * Matrix4::from_angle_x(WEAPON_METER_TILT)
        * Matrix4::from_nonuniform_scale(WEAPON_METER_WIDTH, WEAPON_METER_HEIGHT, 1.0)
}

// --- On-amp psi meter placement -------------------------------------------
//
// The amp_h model is a compact sphere (not an elongated gun body), so the
// psi meter gets its own anchor rather than reusing the on-weapon tag's -
// tuned directly against the amp mesh, near its cable/base on the side
// facing the shooter, so it reads like a status readout on the device
// itself rather than a tag floating off it.

const AMP_METER_ANCHOR: Vector3<f32> = vec3(-0.03, 0.05, 0.0);
/// Panel size in world units, preserving the psi panel's authored 80x32
/// aspect.
const AMP_METER_WIDTH: f32 = 0.08;
const AMP_METER_HEIGHT: f32 = AMP_METER_WIDTH * (psi_amp_panel::PANEL_H / psi_amp_panel::PANEL_W);

/// Root transform for the on-amp psi meter: same canvas convention as
/// [`weapon_meter_transform`] (+Z at the viewer), hung at [`AMP_METER_ANCHOR`]
/// in the amp's local frame.
pub(crate) fn amp_meter_transform(weapon_transform: Matrix4<f32>) -> Matrix4<f32> {
    weapon_transform
        * Matrix4::from_translation(AMP_METER_ANCHOR)
        * Matrix4::from_angle_y(Deg(90.0))
        * Matrix4::from_angle_x(WEAPON_METER_TILT)
        * Matrix4::from_nonuniform_scale(AMP_METER_WIDTH, AMP_METER_HEIGHT, 1.0)
}

/// The on-amp psi meter's root transform, or `None` when the psi amp is not
/// wielded, has nothing to show, or has no transform. Shared by both
/// presentations - VR hangs it directly off the amp; flat premultiplies it by
/// the viewmodel's FOV `squish` (see `mission_core`'s flat viewmodel draw).
pub(crate) fn amp_psi_meter_root(
    world: &World,
) -> Option<(psi_amp_panel::PsiAmpReadout, Matrix4<f32>)> {
    let weapon = crate::wielded_weapon::wielded_weapon(world)?;
    if !crate::wielded_weapon::is_psi_amp(world, weapon) {
        return None;
    }
    let readout = psi_amp_panel::PsiAmpReadout::from_world(world);
    if readout.is_empty() {
        return None;
    }
    let weapon_transform = world
        .borrow::<View<RuntimePropTransform>>()
        .ok()?
        .get(weapon)
        .ok()?
        .0;

    let root = amp_meter_transform(without_scale(weapon_transform));
    Some((readout, root))
}

/// The on-amp psi meter: the psi pool bar and (while charging) the
/// hold-to-overload meter, laid out once in [`psi_amp_panel`], hung off the
/// wielded psi amp's live `RuntimePropTransform` below its ammo/tier tag.
/// Empty when the psi amp is not wielded or has no transform.
pub(crate) fn create_amp_psi_meter(
    asset_cache: &mut AssetCache,
    world: &World,
) -> Vec<SceneObject> {
    let Some((readout, root)) = amp_psi_meter_root(world) else {
        return Vec::new();
    };
    psi_amp_panel::build_readout_canvas(&readout).render_world_space(
        asset_cache,
        root,
        None,
        None,
        OVERLAY_Z_OFFSET,
    )
}

/// A transform's translation + rotation with any (positive) scale stripped:
/// `RuntimePropTransform` bakes `PropScale` in, and a scaled weapon must not
/// scale the meter's authored offset or panel size. (A negative/mirroring
/// scale stays mirrored - no weapon the player wields authors one.)
fn without_scale(m: Matrix4<f32>) -> Matrix4<f32> {
    Matrix4::from_cols(
        m.x.truncate().normalize().extend(0.0),
        m.y.truncate().normalize().extend(0.0),
        m.z.truncate().normalize().extend(0.0),
        m.w,
    )
}

/// The on-weapon ammo meter: the compact AMMOBACK readout (round count + type,
/// the flat HUD's non-use-mode square, laid out once in [`ammo_panel`]) hung
/// off the wielded weapon's live `RuntimePropTransform` (the anchor
/// `systems/attachment.rs` uses for muzzle flashes), behind the gun body like
/// a rear-sight status tag. Empty when nothing with a clip (or the psi amp)
/// is wielded, or the weapon has no transform.
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

    let anchor = crate::vr_config::weapon_meter_anchor_from_entity(world, weapon);
    let root = weapon_meter_transform(without_scale(weapon_transform), anchor);
    let mut objects = vec![virtual_arms::panel_backdrop(
        asset_cache,
        "AMMOBACK.PCX",
        root,
    )];
    objects.append(
        &mut ammo_panel::build_compact_readout_canvas(&readout).render_world_space(
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

/// How well the left wrist panel faces the eye this frame: the dot of the
/// panel's outward normal (local +Z of the shared forearm pose - the same
/// side its overlays stack toward) with the unit vector from the panel to the
/// eye. `None` when the hand pose is untracked (VR rule 7 - see
/// [`crate::util::is_tracked_rotation`]) or the eye is on the panel.
pub(crate) fn wrist_glance_alignment(
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    eye_position: Vector3<f32>,
) -> Option<f32> {
    if !crate::util::is_tracked_rotation(left_hand_rotation) {
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
    use cgmath::{SquareMatrix, vec4};

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
    fn an_untracked_wrist_yields_no_alignment() {
        assert_eq!(
            wrist_glance_alignment(
                vec3(0.0, 0.0, 0.0),
                Quaternion::new(0.0, 0.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ),
            None
        );
    }

    #[test]
    fn the_weapon_meter_hangs_at_its_anchor() {
        let anchor = vec3(0.25, 0.15, 0.0);
        let root = weapon_meter_transform(Matrix4::identity(), anchor);
        // The translation column is exactly the anchor (rotation/scale do not
        // move the panel origin).
        assert_eq!(
            vec3(root.w.x, root.w.y, root.w.z),
            anchor,
            "panel centre must sit at the weapon-local anchor"
        );
    }

    #[test]
    fn the_weapon_meter_faces_back_toward_the_shooter() {
        // The panel's +Z (its outward face) must map dominantly to the
        // weapon's +X - the direction back along the barrel at the shooter.
        let root = weapon_meter_transform(Matrix4::identity(), vec3(0.0, 0.0, 0.0));
        let face = root * vec4(0.0, 0.0, 1.0, 0.0);
        assert!(
            face.x > 0.9,
            "panel normal should point at the shooter (+X), got {face:?}"
        );
    }

    #[test]
    fn a_scaled_weapon_does_not_scale_the_meter() {
        // A weapon with PropScale baked into its transform must not move or
        // resize the meter: the anchor lands at the unscaled offset.
        let scaled = Matrix4::from_nonuniform_scale(2.0, 3.0, 2.0);
        let anchor = vec3(0.25, 0.15, 0.0);
        let root = weapon_meter_transform(without_scale(scaled), anchor);
        assert_eq!(vec3(root.w.x, root.w.y, root.w.z), anchor);
        // ...and the panel's world width stays the authored width.
        let width_axis = root * vec4(1.0, 0.0, 0.0, 0.0);
        assert!((width_axis.truncate().magnitude() - WEAPON_METER_WIDTH).abs() < 1e-5);
    }

    #[test]
    fn the_amp_meter_hangs_at_its_anchor() {
        let root = amp_meter_transform(Matrix4::identity());
        assert_eq!(
            vec3(root.w.x, root.w.y, root.w.z),
            AMP_METER_ANCHOR,
            "panel centre must sit at the amp-local anchor"
        );
    }

    #[test]
    fn the_amp_meter_preserves_its_authored_aspect() {
        let root = amp_meter_transform(Matrix4::identity());
        let width_axis = (root * vec4(1.0, 0.0, 0.0, 0.0)).truncate().magnitude();
        let height_axis = (root * vec4(0.0, 1.0, 0.0, 0.0)).truncate().magnitude();
        assert!((width_axis - AMP_METER_WIDTH).abs() < 1e-5);
        assert!((height_axis - AMP_METER_HEIGHT).abs() < 1e-5);
    }
}
