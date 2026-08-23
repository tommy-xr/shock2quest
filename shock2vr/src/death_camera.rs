//! The death camera: the fall to the floor that plays when the player dies.
//!
//! # Why this lives here and not in the runtimes
//!
//! The rendered view is built from two transforms (`engine::util::compute_view_matrix`):
//!
//! ```text
//! view = inv(head_translation * head_rotation) * inv(pawn_translation * pawn_rotation)
//! ```
//!
//! The pawn comes from [`crate::Game::render`], but the *head* is synthesized
//! per-runtime - the flat runtimes from [`crate::Game::player_eye_height`] plus
//! their look rotation, `oculus_runtime` from the OpenXR view pose mapped into
//! pawn space and capped to the collider crown. Three independent producers.
//!
//! If each runtime blended the death camera into its own head pose, the fall
//! would drift between flat and VR exactly the way AGENTS.md section 3 forbids.
//! So the blend happens once, here, over an already-resolved tracked head pose:
//! a runtime hands in the head it would have rendered with and gets back the
//! head it should render with, making no placement decision of its own.
//!
//! # What it blends
//!
//! Only the *head* component moves. The pawn transform is also the play-space
//! to world map for the hands and the world-space UI panels
//! (`Game::render`'s `pawn_to_world`), so dropping the pawn would drag them to
//! the floor with the camera. Blending the head alone lets the view fall while
//! the world stays where it is.
//!
//! The blend weight is the death pose's authority over the tracked head: at 0
//! the runtime's tracked pose is untouched (bit-identical passthrough, which is
//! every frame the player is alive), at 1 the camera is pinned to the fallen
//! pose.
//!
//! # Stereo
//!
//! [`resolve`] takes the head *centre*, not a per-eye pose. A stereo runtime
//! renders two eyes displaced from that centre by half the IPD, and the fallen
//! target is a single eye-independent point - so blending each eye toward it
//! directly would converge them, shrinking the IPD to zero over the fall and
//! leaving the settled corpse view monoscopic. A depth cue dissolving under you
//! is its own discomfort trigger, on top of losing stereo exactly when the view
//! is most disorienting. So VR resolves once from the centre and re-applies its
//! eye offset with [`reapply_eye_offset`], which rotates with the camera so the
//! eyes stay level with the fallen horizon.

use cgmath::{
    InnerSpace, Matrix3, Matrix4, One, Quaternion, Rotation, Vector2, Vector3, VectorSpace, vec3,
};

/// How long the fall to the floor takes, in seconds. Comfortably inside the
/// shortest death window it plays in (the 3 s terminal-death sequence), so the
/// camera is settled before the game-over screen takes over.
pub const FALL_SECONDS: f32 = 0.9;

/// Where the fallen eye ends up above the floor, in world units. Kept well
/// clear of the 0.1 world-unit near plane so the floor does not clip through
/// the camera once it lands.
pub const FALLEN_EYE_HEIGHT: f32 = 0.45;

/// How far the eye drifts horizontally as it falls, in world units - the body
/// toppling over rather than sinking straight down.
const FALL_LATERAL: f32 = 0.75;

/// How long, after the fall settles, the tracked head takes to regain its
/// influence over the view.
///
/// A fully pinned camera is the single most uncomfortable state in VR: the
/// horizon stops answering head movement, which reads as the world moving
/// rather than the player. The fall itself is short enough to be a deliberate
/// jolt, but the corpse then lies there for the rest of the death window, so
/// the tracked head is let back in as a *delta* on the fallen frame - the
/// player can look around from where they fell, and the punishing part (being
/// on the floor, on your side, unable to act) is untouched.
const LOOK_RECOVERY_SECONDS: f32 = 0.6;

/// An eye pose in pawn space: the same space the runtimes' `head_offset` and
/// `head_rotation` live in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyePose {
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

/// The full camera a runtime renders from, in the terms it hands to
/// `engine::EngineRenderContext`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    pub pawn_position: Vector3<f32>,
    pub pawn_rotation: Quaternion<f32>,
    pub head_offset: Vector3<f32>,
    pub head_rotation: Quaternion<f32>,
}

impl CameraPose {
    /// Build the render context a runtime submits for this camera. The four
    /// fields of a `CameraPose` *are* the first four of an
    /// `EngineRenderContext`, and every runtime pairs them with the same three
    /// per-frame values, so the unpack lives here once rather than being
    /// retyped (and drifting) in each runtime.
    pub fn into_render_context(
        self,
        time: f32,
        projection_matrix: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> engine::EngineRenderContext {
        engine::EngineRenderContext {
            time,
            camera_offset: self.pawn_position,
            camera_rotation: self.pawn_rotation,
            head_offset: self.head_offset,
            head_rotation: self.head_rotation,
            projection_matrix,
            screen_size,
        }
    }
}

/// One frame of the death camera: where the fallen eye sits, and how much
/// authority it has over the runtime's tracked head pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathCameraSample {
    pub eye: EyePose,
    /// 0 = the tracked head is authoritative, 1 = the fallen eye is.
    pub weight: f32,
    /// The tracked head rotation at the moment of death, and how much of the
    /// player's movement away from it to re-admit (0 during the fall, ramping
    /// to 1 once it settles). See [`LOOK_RECOVERY_SECONDS`].
    pub look_frame: Quaternion<f32>,
    pub look_recovery: f32,
}

/// The fall itself: a target pose chosen once when the player dies, and the
/// elapsed time that ramps its authority in.
///
/// The fall is expressed purely as a *weight*, never as a remembered start
/// pose: [`resolve`] blends from whatever the tracked head is doing right now,
/// so a VR player still moving their head during the fall is carried along
/// smoothly instead of being snapped to a stale snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct DeathCamera {
    target: EyePose,
    /// The tracked head rotation this death started from, so the player's later
    /// head movement can be measured against it rather than against the fallen
    /// frame it knows nothing about.
    look_frame: Quaternion<f32>,
    elapsed_seconds: f32,
}

impl DeathCamera {
    /// Choose where this death lands. `live_eye` is the eye pose at the moment
    /// of death and `floor_y` the pawn-space height of the surface underfoot
    /// (`-Game::player_center_above_floor()`). `seed` picks the fall direction
    /// and is the only source of randomness, so a replayed death - a captured
    /// screenshot sequence, an e2e assertion - falls exactly the same way.
    pub fn begin(live_eye: EyePose, floor_y: f32, seed: u64, lateral: f32) -> Self {
        // The body topples SIDEWAYS from where the player was looking, rather
        // than onto a freely random heading: whatever killed you stays in
        // frame while you go down. A random heading reads as the view being
        // yanked away from the fight, and hides the one thing the player most
        // wants to see. The seed picks which side you fall on - that is the
        // randomness, and it is what keeps two deaths from looking identical.
        let forward = death_facing(live_eye.rotation);
        // Up leaves world-up for the horizontal perpendicular to the gaze: the
        // view rolls onto its side. Forward stays horizontal, so the fallen
        // camera looks along the floor rather than into it.
        let up = topple_direction(live_eye.rotation, seed);
        let right = forward.cross(up);
        // `head_rotation` maps head-local axes into pawn space, and cgmath's
        // camera convention looks down local -Z.
        let rotation = Quaternion::from(Matrix3::from_cols(right, up, -forward));

        // `lateral` is how far the body may actually topple - the caller
        // shortens it when something is in the way (see
        // `MissionCore::begin_death_camera`). Dying with your back to a wall is
        // the common case when cornered, and an unclamped drift would put the
        // camera inside it.
        let lateral = if lateral.is_finite() {
            lateral.clamp(0.0, FALL_LATERAL)
        } else {
            0.0
        };
        let target = EyePose {
            position: vec3(
                live_eye.position.x + up.x * lateral,
                floor_y + FALLEN_EYE_HEIGHT,
                live_eye.position.z + up.z * lateral,
            ),
            rotation,
        };

        DeathCamera {
            target,
            look_frame: live_eye.rotation,
            elapsed_seconds: 0.0,
        }
    }

    pub fn advance(&mut self, delta_seconds: f32) {
        if delta_seconds.is_finite() && delta_seconds > 0.0 {
            self.elapsed_seconds += delta_seconds;
        }
    }

    pub fn sample(&self) -> DeathCameraSample {
        DeathCameraSample {
            eye: self.target,
            weight: ease_in_out(self.elapsed_seconds / FALL_SECONDS),
            look_frame: self.look_frame,
            look_recovery: ease_in_out(
                (self.elapsed_seconds - FALL_SECONDS) / LOOK_RECOVERY_SECONDS,
            ),
        }
    }
}

/// Fold the death camera's contribution into the camera a runtime would
/// otherwise have rendered with.
///
/// With no sample - which is every frame the player is alive - this returns the
/// inputs verbatim, so the alive path is bit-identical to reading the tracked
/// pose directly.
pub fn resolve(
    pawn_position: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    tracked_head_offset: Vector3<f32>,
    tracked_head_rotation: Quaternion<f32>,
    sample: Option<DeathCameraSample>,
) -> CameraPose {
    let passthrough = CameraPose {
        pawn_position,
        pawn_rotation,
        head_offset: tracked_head_offset,
        head_rotation: tracked_head_rotation,
    };

    let Some(sample) = sample else {
        return passthrough;
    };
    if !sample.weight.is_finite() || sample.weight <= 0.0 {
        return passthrough;
    }
    let weight = sample.weight.min(1.0);

    CameraPose {
        head_offset: tracked_head_offset.lerp(sample.eye.position, weight),
        head_rotation: slerp_safe(
            tracked_head_rotation,
            fallen_rotation(&sample, tracked_head_rotation),
            weight,
        ),
        ..passthrough
    }
}

/// The fallen frame, with as much of the player's head movement since they died
/// re-admitted as [`DeathCameraSample::look_recovery`] allows.
///
/// The movement is measured against the tracked rotation at death rather than
/// applied absolutely: the fallen view is a new frame of reference, and what
/// carries over is how far the player has turned since, not where they were
/// facing before.
fn fallen_rotation(
    sample: &DeathCameraSample,
    tracked_head_rotation: Quaternion<f32>,
) -> Quaternion<f32> {
    let recovery = if sample.look_recovery.is_finite() {
        sample.look_recovery.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if recovery <= 0.0 {
        return sample.eye.rotation;
    }
    let (Some(look_frame), Some(tracked)) = (unit(sample.look_frame), unit(tracked_head_rotation))
    else {
        return sample.eye.rotation;
    };
    let delta = slerp_safe(
        Quaternion::one(),
        tracked * look_frame.conjugate(),
        recovery,
    );
    // LEFT-multiplied, deliberately. Right-multiplying would apply the turn in
    // the fallen view's own frame, where the roll has taken "up" onto a
    // horizontal axis - so the player's real yaw (which their inner ear feels
    // as rotation about world up) would come back as visual ROLL. That
    // vestibular-visual axis mismatch is a stronger nausea source than the
    // pinned camera this recovery exists to avoid. Left-multiplying keeps head
    // yaw producing view yaw about world up.
    delta * sample.eye.rotation
}

/// The unit form of a rotation, or `None` for a degenerate one that carries no
/// rotation information.
fn unit(rotation: Quaternion<f32>) -> Option<Quaternion<f32>> {
    (rotation.magnitude2() > 1.0e-6).then(|| rotation.normalize())
}

/// `Quaternion::slerp` is only defined for unit quaternions and produces NaN
/// for a degenerate one - which would poison the view matrix and blank the
/// frame. A zero-length input is treated as "no rotation information", so the
/// blend falls back to the other end rather than to garbage.
fn slerp_safe(from: Quaternion<f32>, to: Quaternion<f32>, weight: f32) -> Quaternion<f32> {
    match (unit(from), unit(to)) {
        (Some(from), Some(to)) => from.slerp(to, weight),
        (None, Some(to)) => to,
        (Some(from), None) => from,
        (None, None) => Quaternion::one(),
    }
}

/// The fall's ease over its normalized duration. Named rather than inlined so
/// the feel (a settle bounce at the end) can be tuned in one place.
fn ease_in_out(t: f32) -> f32 {
    crate::util::smoothstep(t)
}

/// Which side the body falls on, from the seed - splitmix64 avalanched down to
/// its top bit. A seedable RNG (and its cargo feature) would be a lot of
/// machinery for one coin flip.
fn seed_is_left(seed: u64) -> bool {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    z >> 63 == 1
}

/// The horizontal direction the body topples in: perpendicular to where the
/// player was looking, on the side the seed picks. Public so the caller can
/// sweep it for obstructions before deciding how far the fall may carry (see
/// [`DeathCamera::begin`]'s `lateral`).
pub fn topple_direction(head_rotation: Quaternion<f32>, seed: u64) -> Vector3<f32> {
    let forward = death_facing(head_rotation);
    let side = if seed_is_left(seed) { 1.0 } else { -1.0 };
    vec3(-forward.z, 0.0, forward.x) * side
}

/// How far the body topples when nothing is in the way, in world units.
pub const MAX_FALL_LATERAL: f32 = FALL_LATERAL;

/// The horizontal direction the player was facing when they died. A gaze that
/// is straight up or down (or an untracked head) has no horizontal component to
/// fall away from, so it falls back to the pawn's forward axis rather than
/// normalizing a zero vector into NaN.
fn death_facing(head_rotation: Quaternion<f32>) -> Vector3<f32> {
    let fallback = vec3(0.0, 0.0, -1.0);
    let Some(rotation) = crate::util::tracked_rotation(head_rotation) else {
        return fallback;
    };
    let gaze = rotation.rotate_vector(fallback);
    let flat = vec3(gaze.x, 0.0, gaze.z);
    if flat.magnitude2() < 1.0e-6 {
        fallback
    } else {
        flat.normalize()
    }
}

/// Re-apply a stereo eye's displacement from the head centre to a resolved
/// camera, so the two eyes keep their separation through the fall instead of
/// converging on one point (see the module's Stereo section).
///
/// `eye_offset_from_centre` is the eye's displacement in the same space the
/// tracked head pose was given in; it is carried into the resolved camera's
/// frame so the eyes roll with the fallen horizon rather than staying level
/// with the room.
pub fn reapply_eye_offset(
    camera: CameraPose,
    eye_offset_from_centre: Vector3<f32>,
    tracked_head_rotation: Quaternion<f32>,
) -> CameraPose {
    let Some(tracked) = crate::util::tracked_rotation(tracked_head_rotation) else {
        return camera;
    };
    let in_head_space = tracked.invert().rotate_vector(eye_offset_from_centre);
    CameraPose {
        head_offset: camera.head_offset + camera.head_rotation.rotate_vector(in_head_space),
        ..camera
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{AbsDiffEq, Deg, Rad, Rotation, Rotation3, Zero};

    /// Angle between two rotations, in degrees. Compared this way rather than
    /// componentwise because `q` and `-q` are the same rotation, and `slerp`
    /// is free to return either.
    fn angle_between(a: Quaternion<f32>, b: Quaternion<f32>) -> f32 {
        let dot = (a.normalize().dot(b.normalize())).abs().min(1.0);
        Deg::from(Rad(2.0 * dot.acos())).0
    }

    fn live_eye() -> EyePose {
        EyePose {
            position: vec3(0.0, 2.6, 0.0),
            rotation: Quaternion::one(),
        }
    }

    fn camera(sample: Option<DeathCameraSample>) -> CameraPose {
        resolve(
            vec3(10.0, 1.0, -4.0),
            Quaternion::from_angle_y(Deg(30.0)),
            live_eye().position,
            live_eye().rotation,
            sample,
        )
    }

    #[test]
    fn no_sample_passes_the_tracked_camera_through_untouched() {
        let resolved = camera(None);
        assert_eq!(resolved.pawn_position, vec3(10.0, 1.0, -4.0));
        assert_eq!(resolved.pawn_rotation, Quaternion::from_angle_y(Deg(30.0)));
        assert_eq!(resolved.head_offset, live_eye().position);
        assert_eq!(resolved.head_rotation, live_eye().rotation);
    }

    #[test]
    fn a_zero_weight_sample_is_also_an_exact_passthrough() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        let sample = DeathCameraSample {
            weight: 0.0,
            ..death.sample()
        };
        assert_eq!(camera(Some(sample)), camera(None));
    }

    #[test]
    fn a_non_finite_weight_falls_back_to_the_tracked_camera() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        for weight in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let sample = DeathCameraSample {
                weight,
                ..death.sample()
            };
            assert_eq!(camera(Some(sample)), camera(None), "weight {weight}");
        }
    }

    #[test]
    fn full_weight_pins_the_head_to_the_fallen_eye() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        let sample = DeathCameraSample {
            weight: 1.0,
            ..death.sample()
        };
        let resolved = camera(Some(sample));
        assert!(
            resolved
                .head_offset
                .abs_diff_eq(&sample.eye.position, 1.0e-5)
        );
        assert!(
            resolved
                .head_rotation
                .abs_diff_eq(&sample.eye.rotation, 1.0e-5)
        );
    }

    #[test]
    fn the_pawn_transform_is_never_touched() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        for weight in [0.0, 0.25, 0.5, 1.0] {
            let resolved = camera(Some(DeathCameraSample {
                weight,
                ..death.sample()
            }));
            assert_eq!(resolved.pawn_position, vec3(10.0, 1.0, -4.0));
            assert_eq!(resolved.pawn_rotation, Quaternion::from_angle_y(Deg(30.0)));
        }
    }

    #[test]
    fn the_eye_falls_monotonically_toward_the_floor() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        let mut previous = camera(Some(death.sample())).head_offset.y;
        for _ in 0..60 {
            death.advance(FALL_SECONDS / 60.0);
            let height = camera(Some(death.sample())).head_offset.y;
            assert!(
                height <= previous + 1.0e-5,
                "{height} rose above {previous}"
            );
            previous = height;
        }
        assert!(
            (previous - (-1.6 + FALLEN_EYE_HEIGHT)).abs() < 1.0e-3,
            "settled at {previous}"
        );
    }

    #[test]
    fn the_weight_saturates_rather_than_overshooting() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        death.advance(FALL_SECONDS * 10.0);
        assert_eq!(death.sample().weight, 1.0);
    }

    #[test]
    fn the_fallen_eye_stays_clear_of_the_near_plane() {
        for seed in 0..64 {
            let death = DeathCamera::begin(live_eye(), -1.6, seed, FALL_LATERAL);
            assert!(death.sample().eye.position.y > -1.6 + 0.1);
        }
    }

    #[test]
    fn the_fallen_view_lies_on_its_side_looking_along_the_floor() {
        for seed in 0..64 {
            let death = DeathCamera::begin(live_eye(), -1.6, seed, FALL_LATERAL);
            let rotation = death.sample().eye.rotation;
            let up = rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
            let forward = rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
            assert!(up.y.abs() < 1.0e-4, "seed {seed}: up is {up:?}");
            assert!(
                forward.y.abs() < 1.0e-4,
                "seed {seed}: forward is {forward:?}"
            );
            assert!((rotation.magnitude() - 1.0).abs() < 1.0e-4, "seed {seed}");
        }
    }

    #[test]
    fn the_fall_direction_is_seeded_and_reproducible() {
        let a = DeathCamera::begin(live_eye(), -1.6, 11, FALL_LATERAL);
        let b = DeathCamera::begin(live_eye(), -1.6, 11, FALL_LATERAL);
        assert_eq!(a, b);

        // The seed is a coin flip over which side the body lands on, and both
        // sides have to come up - a seed that always fell the same way would
        // make every death look identical.
        let sides: Vec<bool> = (0..32).map(seed_is_left).collect();
        assert!(sides.iter().any(|&left| left), "never falls left");
        assert!(sides.iter().any(|&left| !left), "never falls right");
    }

    #[test]
    fn the_body_topples_sideways_so_the_killer_stays_in_frame() {
        // Looking along +X when killed.
        let facing = Quaternion::from_angle_y(Deg(-90.0));
        let eye = EyePose {
            position: vec3(0.0, 2.6, 0.0),
            rotation: facing,
        };
        for seed in 0..32 {
            let death = DeathCamera::begin(eye, -1.6, seed, FALL_LATERAL);
            let gaze = death
                .sample()
                .eye
                .rotation
                .rotate_vector(vec3(0.0, 0.0, -1.0));
            // Still looking where they were looking - the roll is sideways.
            assert!(
                (gaze.x - 1.0).abs() < 1.0e-4 && gaze.y.abs() < 1.0e-4,
                "seed {seed}: fell away from the killer, gaze {gaze:?}"
            );
            // ...and the body went over to one side of that gaze, not along it.
            let drift = death.target.position - eye.position;
            assert!(
                drift.x.abs() < 1.0e-4 && drift.z.abs() > 0.1,
                "seed {seed}: toppled along the gaze rather than across it, drift {drift:?}"
            );
        }
    }

    #[test]
    fn a_vertical_or_untracked_gaze_still_produces_a_finite_fall() {
        for rotation in [
            Quaternion::from_angle_x(Deg(90.0)),
            Quaternion::from_angle_x(Deg(-90.0)),
            Quaternion::zero(),
        ] {
            let eye = EyePose {
                position: vec3(0.0, 2.6, 0.0),
                rotation,
            };
            let target = DeathCamera::begin(eye, -1.6, 3, FALL_LATERAL).target;
            assert!(
                target.position.x.is_finite()
                    && target.position.z.is_finite()
                    && (target.rotation.magnitude() - 1.0).abs() < 1.0e-4,
                "degenerate gaze {rotation:?} produced {target:?}"
            );
        }
    }

    #[test]
    fn a_stereo_pair_keeps_its_separation_all_the_way_through_the_fall() {
        // Two eyes half an IPD either side of the tracked head centre.
        let ipd = 0.2_f32;
        let centre = vec3(0.0, 2.6, 0.0);
        let tracked_rotation = Quaternion::from_angle_y(Deg(20.0));
        let offsets = [
            tracked_rotation.rotate_vector(vec3(-ipd / 2.0, 0.0, 0.0)),
            tracked_rotation.rotate_vector(vec3(ipd / 2.0, 0.0, 0.0)),
        ];

        let mut death = DeathCamera::begin(
            EyePose {
                position: centre,
                rotation: tracked_rotation,
            },
            -1.6,
            5,
            FALL_LATERAL,
        );
        for _ in 0..=60 {
            let resolved = resolve(
                Vector3::zero(),
                Quaternion::one(),
                centre,
                tracked_rotation,
                Some(death.sample()),
            );
            let eyes: Vec<Vector3<f32>> = offsets
                .iter()
                .map(|offset| reapply_eye_offset(resolved, *offset, tracked_rotation).head_offset)
                .collect();
            let separation = (eyes[1] - eyes[0]).magnitude();
            assert!(
                (separation - ipd).abs() < 1.0e-4,
                "stereo separation drifted to {separation} (want {ipd})"
            );
            death.advance(FALL_SECONDS / 60.0);
        }
    }

    #[test]
    fn the_stereo_eyes_roll_with_the_fallen_horizon() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 5, FALL_LATERAL);
        death.advance(FALL_SECONDS * 2.0);
        let resolved = resolve(
            Vector3::zero(),
            Quaternion::one(),
            live_eye().position,
            live_eye().rotation,
            Some(death.sample()),
        );
        // A settled camera lying on its side separates its eyes vertically: the
        // interocular axis has rolled with the view, so the two images stay
        // level with the fallen horizon rather than with the room.
        let right_eye = reapply_eye_offset(resolved, vec3(0.5, 0.0, 0.0), live_eye().rotation);
        let displacement = right_eye.head_offset - resolved.head_offset;
        assert!(
            displacement.y.abs() > 0.49,
            "the eye offset did not roll with the camera: {displacement:?}"
        );
    }

    #[test]
    fn an_untracked_head_leaves_the_stereo_offset_alone() {
        let death = DeathCamera::begin(live_eye(), -1.6, 5, FALL_LATERAL);
        let resolved = resolve(
            Vector3::zero(),
            Quaternion::one(),
            live_eye().position,
            live_eye().rotation,
            Some(death.sample()),
        );
        assert_eq!(
            reapply_eye_offset(resolved, vec3(0.5, 0.0, 0.0), Quaternion::zero()),
            resolved
        );
    }

    #[test]
    fn the_fall_ignores_head_movement_while_it_plays() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        let settled = DeathCameraSample {
            weight: 1.0,
            look_recovery: 0.0,
            ..death.sample()
        };
        // A player turning their head mid-fall does not steer the fall.
        for turn in [0.0, 45.0, 180.0] {
            let resolved = resolve(
                Vector3::zero(),
                Quaternion::one(),
                Vector3::zero(),
                Quaternion::from_angle_y(Deg(turn)),
                Some(settled),
            );
            assert!(
                angle_between(resolved.head_rotation, settled.eye.rotation) < 0.01,
                "turn {turn} steered the fall"
            );
        }
    }

    #[test]
    fn once_settled_the_player_can_look_around_from_where_they_fell() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        death.advance(FALL_SECONDS + LOOK_RECOVERY_SECONDS);
        let sample = death.sample();
        assert_eq!(sample.weight, 1.0);
        assert_eq!(sample.look_recovery, 1.0);

        // Standing still leaves the fallen view exactly where the fall put it.
        let still = resolve(
            Vector3::zero(),
            Quaternion::one(),
            Vector3::zero(),
            live_eye().rotation,
            Some(sample),
        );
        assert!(
            still
                .head_rotation
                .abs_diff_eq(&sample.eye.rotation, 1.0e-5)
        );

        // Turning the head 90 degrees turns the view by 90 degrees about WORLD
        // UP - the axis the player's inner ear felt it about. Applying it in
        // the fallen frame instead would come back as roll, which is the
        // vestibular mismatch this whole ramp exists to avoid.
        let turned = resolve(
            Vector3::zero(),
            Quaternion::one(),
            Vector3::zero(),
            live_eye().rotation * Quaternion::from_angle_y(Deg(90.0)),
            Some(sample),
        );
        let applied = turned.head_rotation * sample.eye.rotation.conjugate();
        assert!(
            angle_between(applied, Quaternion::from_angle_y(Deg(90.0))) < 0.05,
            "expected a 90 degree turn about world up, got {applied:?}"
        );
        // ...and the fallen horizon is still where the body left it: the view
        // yawed, it did not roll further.
        let up = turned.head_rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
        assert!(up.y.abs() < 1.0e-3, "the turn rolled the view: up {up:?}");
    }

    #[test]
    fn the_look_recovery_only_starts_once_the_fall_has_landed() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        assert_eq!(death.sample().look_recovery, 0.0);
        death.advance(FALL_SECONDS);
        assert_eq!(death.sample().look_recovery, 0.0);
        death.advance(LOOK_RECOVERY_SECONDS / 2.0);
        let partial = death.sample().look_recovery;
        assert!(partial > 0.0 && partial < 1.0, "got {partial}");
        death.advance(LOOK_RECOVERY_SECONDS);
        assert_eq!(death.sample().look_recovery, 1.0);
    }

    #[test]
    fn a_degenerate_look_frame_leaves_the_fallen_view_alone() {
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        death.advance(FALL_SECONDS + LOOK_RECOVERY_SECONDS);
        let sample = DeathCameraSample {
            look_frame: Quaternion::zero(),
            ..death.sample()
        };
        let resolved = resolve(
            Vector3::zero(),
            Quaternion::one(),
            Vector3::zero(),
            Quaternion::from_angle_y(Deg(90.0)),
            Some(sample),
        );
        assert!(
            resolved
                .head_rotation
                .abs_diff_eq(&sample.eye.rotation, 1.0e-5)
        );
    }

    #[test]
    fn a_degenerate_tracked_rotation_does_not_poison_the_blend() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7, FALL_LATERAL);
        let resolved = resolve(
            Vector3::zero(),
            Quaternion::one(),
            Vector3::zero(),
            Quaternion::zero(),
            Some(DeathCameraSample {
                weight: 0.5,
                ..death.sample()
            }),
        );
        assert!(resolved.head_rotation.magnitude().is_finite());
        assert!((resolved.head_rotation.magnitude() - 1.0).abs() < 1.0e-4);
    }
}
