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
//! Flying reuses the player's own locomotion channels, so the controls are
//! the ones the hands already know and no runtime needs its own mapping:
//! right thumbstick strafes and moves along the view, left thumbstick turns
//! (x) and rises/falls (y). On the desktop those are `W`/`A`/`S`/`D` and the
//! arrow keys, and mouse-look aims the camera because the runtimes compose
//! the head rotation onto the camera pose themselves. While detached those
//! channels are withheld from the scene (see `Game::update`), so the pawn
//! stands still rather than sleepwalking off while you fly.

use cgmath::{Matrix4, Quaternion, Rotation, Rotation3, SquareMatrix, Vector3, vec3};

use crate::dev_params;
use crate::input_context::InputContext;
use crate::mission::PLAYER_TURN_RATE;
use crate::time::Time;

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

    /// Fly the camera for one frame. A no-op while attached, and while the
    /// pose has not been captured yet (the detach frame), so the camera never
    /// moves before it has somewhere to move from.
    ///
    /// The motion is `mission_core`'s player locomotion with the physics
    /// removed: the same channels, the same axes, the same turn rate - only
    /// integrated straight into the pose instead of driven through a
    /// character controller. That is what makes it noclip, and deliberately
    /// so: flying through a wall to look at the far side is the tool working.
    pub fn fly(&mut self, time: &Time, input_context: &InputContext) {
        let Some((position, rotation)) = self.pose else {
            return;
        };
        let delta_time = time.elapsed.as_secs_f32();
        if delta_time == 0.0 {
            // A paused sim must not drift, and the debug runtime pauses by
            // calling update with a zero dt.
            return;
        }

        let turn = input_context.left_hand.thumbstick.x * delta_time * PLAYER_TURN_RATE;
        let rotation =
            rotation * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), cgmath::Rad(turn));

        // The head rotation is composed in for the *direction of travel* only.
        // It is deliberately not stored: the runtimes apply it to the camera
        // pose themselves when they build the view matrix, so keeping it would
        // apply a tracked headset's rotation twice.
        let facing = rotation * input_context.head.rotation;
        let speed = dev_params::get(dev_params::FREE_CAMERA_SPEED) / dark::SCALE_FACTOR;
        let step = delta_time * speed;
        let move_thumbstick = input_context.right_hand.thumbstick;
        let position = position
            + facing.rotate_vector(vec3(
                -step * move_thumbstick.x,
                0.0,
                -step * move_thumbstick.y,
            ))
            + vec3(0.0, step * input_context.left_hand.thumbstick.y, 0.0);

        self.pose = Some((position, rotation));
    }

    /// Whether the scene should be denied this frame's locomotion input,
    /// because the free camera is consuming it. The pawn keeps everything
    /// else - it can still be looked at, damaged, and scripted - it just does
    /// not walk while the sticks are flying the camera.
    pub fn consumes_locomotion(&self) -> bool {
        self.detached
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

/// The input context with the channels that move the body zeroed - what the
/// scene sees while the free camera is flying on them. A copy rather than a
/// mutation so the runtime's own context is left intact for the next frame,
/// and so the hands keep their poses, triggers and grips: only moving the
/// body is withheld, and the body can still be aimed, fired and inspected.
///
/// `crouch` is withheld along with the sticks, even though it is not
/// locomotion, because the flat runtimes build their camera `head_offset`
/// from the crouch-aware [`Game::player_eye_height`]: left through, it would
/// drop the supposedly frozen camera by the crouch delta. (In VR the tracked
/// head legitimately moves the view and never goes through that accessor.)
///
/// [`Game::player_eye_height`]: crate::Game::player_eye_height
pub fn without_locomotion(input_context: &InputContext) -> InputContext {
    let mut withheld = input_context.clone();
    withheld.left_hand.thumbstick = cgmath::Vector2::new(0.0, 0.0);
    withheld.right_hand.thumbstick = cgmath::Vector2::new(0.0, 0.0);
    withheld.jump = false;
    withheld.crouch = false;
    withheld
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

    fn one_second() -> Time {
        Time {
            elapsed: std::time::Duration::from_secs(1),
            total: std::time::Duration::from_secs(1),
        }
    }

    /// A camera detached and flown forward for a second. Returns its pose.
    fn flown(stick: impl Fn(&mut InputContext)) -> Pose {
        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose((vec3(0.0, 0.0, 0.0), Quaternion::from_angle_y(Deg(0.0_f32))));
        let mut input = InputContext::default();
        input.head.rotation = Quaternion::from_angle_y(Deg(0.0_f32));
        stick(&mut input);
        camera.fly(&one_second(), &input);
        camera.camera_pose((
            vec3(99.0, 99.0, 99.0),
            Quaternion::from_angle_y(Deg(0.0_f32)),
        ))
    }

    /// Forward on the right stick moves along -Z, the same axis the player
    /// walks along in `mission_core`.
    #[test]
    fn the_right_stick_flies_along_the_view() {
        let (position, _) = flown(|input| input.right_hand.thumbstick.y = 1.0);
        assert!(position.z < -0.1, "expected -Z travel, got {position:?}");
        assert!(position.x.abs() < 1e-4 && position.y.abs() < 1e-4);
    }

    /// The left stick's y is vertical, in world space - flying up is up
    /// however the camera is pitched.
    #[test]
    fn the_left_stick_y_flies_vertically() {
        let (position, _) = flown(|input| input.left_hand.thumbstick.y = 1.0);
        assert!(position.y > 0.1, "expected +Y travel, got {position:?}");
        assert!(position.x.abs() < 1e-4 && position.z.abs() < 1e-4);
    }

    /// The left stick's x turns and does not translate.
    #[test]
    fn the_left_stick_x_turns_in_place() {
        let (position, rotation) = flown(|input| input.left_hand.thumbstick.x = 1.0);
        assert!(position.magnitude() < 1e-4, "turning must not move");
        let facing = rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!(facing.x.abs() > 0.1, "expected a yaw, got {facing:?}");
        assert!(facing.y.abs() < 1e-4, "yaw must not pitch the camera");
    }

    /// An attached camera ignores the sticks entirely - the player is walking
    /// on them.
    #[test]
    fn an_attached_camera_does_not_fly() {
        let mut camera = FreeCamera::new();
        let mut input = InputContext::default();
        input.right_hand.thumbstick.y = 1.0;
        camera.fly(&one_second(), &input);
        assert_eq!(camera.camera_pose(pawn()), pawn());
    }

    /// A paused sim must not drift: the debug runtime pauses by calling
    /// update with a zero dt.
    #[test]
    fn a_zero_timestep_does_not_move_the_camera() {
        let mut camera = FreeCamera::new();
        camera.toggle();
        camera.camera_pose(pawn());
        let mut input = InputContext::default();
        input.right_hand.thumbstick.y = 1.0;
        camera.fly(
            &Time {
                elapsed: std::time::Duration::ZERO,
                total: std::time::Duration::from_secs(1),
            },
            &input,
        );
        assert_eq!(camera.camera_pose(pawn()), pawn());
    }

    /// Withholding must take the channels that move the body and nothing
    /// else: the hands keep working, so the body can still be aimed and fired
    /// while the camera watches from outside.
    #[test]
    fn withholding_locomotion_keeps_everything_but_walking() {
        let mut input = InputContext::default();
        input.left_hand.thumbstick = cgmath::Vector2::new(1.0, 1.0);
        input.right_hand.thumbstick = cgmath::Vector2::new(1.0, 1.0);
        input.jump = true;
        input.crouch = true;
        input.right_hand.trigger_value = 1.0;
        input.head.position = vec3(0.0, 5.6, 0.0);

        let withheld = without_locomotion(&input);
        assert_eq!(
            withheld.left_hand.thumbstick,
            cgmath::Vector2::new(0.0, 0.0)
        );
        assert_eq!(
            withheld.right_hand.thumbstick,
            cgmath::Vector2::new(0.0, 0.0)
        );
        assert!(!withheld.jump);
        // Crouch moves the flat runtimes' camera through the crouch-aware eye
        // height, so a "frozen" camera must not see it either.
        assert!(!withheld.crouch);
        assert_eq!(withheld.right_hand.trigger_value, 1.0);
        assert_eq!(withheld.head.position, input.head.position);
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
