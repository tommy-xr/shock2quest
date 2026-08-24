//! The detached debug ("free") camera.
//!
//! A render-layer override and nothing more: while it is detached the runtime
//! draws from a camera pose of its own, but the player pawn does not move, is
//! not teleported, and never learns the camera left. Everything that reads the
//! player's position - AI (`PlayerInfo.pos`), triggers, audio - therefore keeps
//! reading the *body*, which is the point: the camera is a way to watch the
//! simulation from outside without perturbing it.
//!
//! Two switches gate it, both [`crate::dev_params`] bools:
//!
//! * `free_camera` is the enable. Until it is on, the toggle input is not even
//!   read, so a stray keypress or controller chord during normal play cannot
//!   detach the view. Turning it back off re-attaches (see [`FreeCamera::sync_gate`]).
//! * `free_camera_cull` picks which pose the visibility engine culls from
//!   while detached - the player (default) or the camera. See
//!   [`FreeCamera::cull_from_camera`].
//!
//! Movement is not implemented yet: the camera snaps to the eye it detached
//! from and holds that pose.

use cgmath::{Matrix4, Quaternion, SquareMatrix, Vector3};

use crate::dev_params;

/// A camera pose: where it is, and which way it faces.
pub type Pose = (Vector3<f32>, Quaternion<f32>);

/// The free camera's state. Attached (the default) is `pose: None`; the pose
/// is captured on the first frame rendered after detaching, so the camera
/// always starts exactly where the player's eye was and the toggle reads as a
/// freeze rather than a jump.
#[derive(Debug, Default, Clone)]
pub struct FreeCamera {
    detached: bool,
    /// The captured pose. `None` while attached, and also for the single
    /// frame between a detach and the render that captures it.
    pose: Option<Pose>,
}

impl FreeCamera {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the camera is currently detached from the player.
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    /// Act on the toggle input. Detaching only *arms* the camera; the pose is
    /// captured by the next [`camera_pose`], which is the one place that knows
    /// where the eye actually is.
    ///
    /// [`camera_pose`]: Self::camera_pose
    pub fn toggle(&mut self) {
        if self.detached {
            self.attach();
        } else {
            self.detached = true;
        }
    }

    /// Re-attach to the player, dropping the override. Nothing has to be
    /// restored: the pawn never moved.
    pub fn attach(&mut self) {
        self.detached = false;
        self.pose = None;
    }

    /// The captured pose, or `None` while attached (or on the single frame
    /// between a detach and the render that captures it).
    pub fn pose(&self) -> Option<Pose> {
        self.pose
    }

    /// Enforce the `free_camera` dev param every frame, so turning the switch
    /// off on the Developer screen is always a way back to the body - including
    /// from a camera flown somewhere the menu is hard to reach.
    pub fn sync_gate(&mut self) {
        if !Self::is_enabled() && self.is_detached() {
            self.attach();
        }
    }

    /// Whether the free camera is enabled at all.
    pub fn is_enabled() -> bool {
        dev_params::get_bool(dev_params::FREE_CAMERA)
    }

    /// Whether culling should follow the detached camera rather than the
    /// player. Off by default: culling from the player is what makes the
    /// camera a tool for *inspecting* visibility rather than just a way to
    /// fly around.
    pub fn cull_from_camera() -> bool {
        dev_params::get_bool(dev_params::FREE_CAMERA_CULL_FROM_CAMERA)
    }

    /// The pose to render this frame from, given the player's own. Captures
    /// the pawn pose on the first call after a detach.
    pub fn camera_pose(&mut self, pawn: Pose) -> Pose {
        if !self.detached {
            return pawn;
        }
        *self.pose.get_or_insert(pawn)
    }

    /// The correction that turns the *camera's* view matrix back into the
    /// *player's*, or `None` when the two are the same view (attached, or
    /// deliberately culling from the camera).
    ///
    /// Returned as a right-multiplied fixup rather than a rebuilt view matrix
    /// because only the runtime knows the rest of the chain - in VR the view
    /// also carries the tracked head offset, which `Game` never sees. Since
    /// both views share that prefix (`view = head * world_to_eye`), composing
    /// `camera_to_world * world_to_pawn` on the right cancels the camera and
    /// substitutes the pawn exactly, whatever the prefix was.
    pub fn view_fixup(&self, pawn: Pose) -> Option<Matrix4<f32>> {
        let camera = self.pose?;
        if !self.detached || Self::cull_from_camera() {
            return None;
        }
        let camera_to_world = Matrix4::from_translation(camera.0) * Matrix4::from(camera.1);
        let pawn_to_world = Matrix4::from_translation(pawn.0) * Matrix4::from(pawn.1);
        Some(camera_to_world * pawn_to_world.invert()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, InnerSpace, Rad, Rotation3, Vector4, vec3};

    fn pawn() -> Pose {
        (
            vec3(3.0, 5.6, -2.0),
            Quaternion::from_angle_y(Deg(35.0_f32)),
        )
    }

    fn elsewhere() -> Pose {
        (
            vec3(-40.0, 22.0, 17.0),
            Quaternion::from_angle_x(Deg(-20.0_f32)),
        )
    }

    /// Attached, the override is the identity: the runtime gets exactly the
    /// pose the scene produced.
    #[test]
    fn an_attached_camera_passes_the_pawn_pose_through() {
        let mut camera = FreeCamera::new();
        assert!(!camera.is_detached());
        assert_eq!(camera.camera_pose(pawn()).0, pawn().0);
        assert!(camera.view_fixup(pawn()).is_none());
    }

    /// Detaching freezes the view where the eye already was - no jump on the
    /// frame the toggle lands.
    #[test]
    fn detaching_captures_the_pose_it_detached_from() {
        let mut camera = FreeCamera::new();
        camera.toggle();
        assert!(camera.is_detached());
        assert_eq!(camera.camera_pose(pawn()), pawn());
    }

    /// Once captured, the camera holds its pose while the player walks away.
    /// This is the property the whole tool rests on.
    #[test]
    fn a_detached_camera_does_not_follow_the_player() {
        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose(pawn());
        assert_eq!(camera.camera_pose(elsewhere()), pawn());
    }

    #[test]
    fn re_attaching_snaps_back_to_the_player_and_re_arms() {
        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose(pawn());

        camera.toggle();
        assert!(!camera.is_detached());
        assert_eq!(camera.camera_pose(elsewhere()), elsewhere());

        // A second detach captures the NEW pose, not the stale one.
        camera.toggle();
        assert_eq!(camera.camera_pose(elsewhere()), elsewhere());
    }

    /// The dev-param gate is a way back: switching it off re-attaches even
    /// though nothing pressed the toggle.
    #[test]
    fn the_gate_re_attaches_a_detached_camera() {
        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose(pawn());
        // `free_camera` defaults off and no test mutates the process-global
        // registry, so this is the "switch turned off while detached" case.
        assert!(!FreeCamera::is_enabled());
        camera.sync_gate();
        assert!(!camera.is_detached());
    }

    /// The point of the fixup: composed onto the camera's view matrix it must
    /// reproduce the *player's* view matrix exactly - including the tracked
    /// head offset in the prefix, which `Game` never sees.
    #[test]
    fn the_view_fixup_reproduces_the_players_view() {
        let head_position = vec3(0.1, 1.7, -0.3);
        let head_rotation = Quaternion::from_angle_z(Rad(0.2_f32));

        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose(elsewhere());

        let camera_view = engine::util::compute_view_matrix(
            elsewhere().0,
            elsewhere().1,
            head_position,
            head_rotation,
        );
        let expected =
            engine::util::compute_view_matrix(pawn().0, pawn().1, head_position, head_rotation);

        let fixup = camera
            .view_fixup(pawn())
            .expect("a detached camera culling from the player has a fixup");
        let corrected = camera_view * fixup;

        let columns = [
            (corrected.x, expected.x),
            (corrected.y, expected.y),
            (corrected.z, expected.z),
            (corrected.w, expected.w),
        ];
        for (actual_column, expected_column) in columns {
            let difference: Vector4<f32> = actual_column - expected_column;
            assert!(
                difference.magnitude() < 1e-4,
                "corrected view {corrected:?} != player view {expected:?}"
            );
        }
    }
}
