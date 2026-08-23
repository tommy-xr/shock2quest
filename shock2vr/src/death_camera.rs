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

use cgmath::{InnerSpace, Matrix3, One, Quaternion, Rad, Vector3, VectorSpace, vec3};

/// How long the fall to the floor takes, in seconds. Comfortably inside the
/// shortest death window it plays in (the 3 s terminal-death sequence), so the
/// camera is settled before the game-over screen takes over.
pub const FALL_SECONDS: f32 = 0.9;

/// Where the fallen eye ends up above the floor, in world units. Kept well
/// clear of the 0.1 world-unit near plane so the floor does not clip through
/// the camera once it lands.
const FALLEN_EYE_HEIGHT: f32 = 0.45;

/// How far the eye drifts horizontally as it falls, in world units - the body
/// toppling over rather than sinking straight down.
const FALL_LATERAL: f32 = 0.75;

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

/// One frame of the death camera: where the fallen eye sits, and how much
/// authority it has over the runtime's tracked head pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathCameraSample {
    pub eye: EyePose,
    /// 0 = the tracked head is authoritative, 1 = the fallen eye is.
    pub weight: f32,
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
    elapsed_seconds: f32,
}

impl DeathCamera {
    /// Choose where this death lands. `live_eye` is the eye pose at the moment
    /// of death and `floor_y` the pawn-space height of the surface underfoot
    /// (`-Game::player_center_above_floor()`). `seed` picks the fall direction
    /// and is the only source of randomness, so a replayed death - a captured
    /// screenshot sequence, an e2e assertion - falls exactly the same way.
    pub fn begin(live_eye: EyePose, floor_y: f32, seed: u64) -> Self {
        let azimuth = Rad(unit_from_seed(seed) * std::f32::consts::TAU);
        let (sin, cos) = azimuth.0.sin_cos();

        // "Up" ends up pointing along a random horizontal direction: the view
        // rolls onto its side as the body topples. Forward stays horizontal
        // (world up crossed with the new up), so the fallen camera looks along
        // the floor rather than into it.
        let up = vec3(sin, 0.0, cos);
        let forward = vec3(cos, 0.0, -sin);
        let right = forward.cross(up);
        // `head_rotation` maps head-local axes into pawn space, and cgmath's
        // camera convention looks down local -Z.
        let rotation = Quaternion::from(Matrix3::from_cols(right, up, -forward));

        let target = EyePose {
            position: vec3(
                live_eye.position.x + up.x * FALL_LATERAL,
                floor_y + FALLEN_EYE_HEIGHT,
                live_eye.position.z + up.z * FALL_LATERAL,
            ),
            rotation,
        };

        DeathCamera {
            target,
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
        head_rotation: slerp_safe(tracked_head_rotation, sample.eye.rotation, weight),
        ..passthrough
    }
}

/// `Quaternion::slerp` is only defined for unit quaternions and produces NaN
/// for a degenerate one - which would poison the view matrix and blank the
/// frame. A zero-length input is treated as "no rotation information", so the
/// blend falls back to the other end rather than to garbage.
fn slerp_safe(from: Quaternion<f32>, to: Quaternion<f32>, weight: f32) -> Quaternion<f32> {
    let normalize = |q: Quaternion<f32>| {
        if q.magnitude2() > 1.0e-6 {
            Some(q.normalize())
        } else {
            None
        }
    };
    match (normalize(from), normalize(to)) {
        (Some(from), Some(to)) => from.slerp(to, weight),
        (None, Some(to)) => to,
        (Some(from), None) => from,
        (None, None) => Quaternion::one(),
    }
}

/// Smoothstep over the normalized fall. Named rather than inlined so the feel
/// (a settle bounce at the end) can be tuned in one place.
fn ease_in_out(t: f32) -> f32 {
    if !t.is_finite() || t <= 0.0 {
        return 0.0;
    }
    let t = t.min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A deterministic [0, 1) draw from a seed - splitmix64, avalanched and taken
/// from the high bits. Keeps the fall direction reproducible without pulling a
/// seedable RNG (and its cargo feature) in for a single number.
fn unit_from_seed(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    ((z >> 40) as f32) / ((1u32 << 24) as f32)
}

/// The eye pose a live, upright player renders from, for callers that need a
/// neutral starting pose (tests, and the death-camera hand-off).
pub fn upright_eye(eye_height: f32) -> EyePose {
    EyePose {
        position: vec3(0.0, eye_height, 0.0),
        rotation: Quaternion::one(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{AbsDiffEq, Deg, Rotation, Rotation3, Zero};

    fn live_eye() -> EyePose {
        upright_eye(2.6)
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
        let death = DeathCamera::begin(live_eye(), -1.6, 7);
        let sample = DeathCameraSample {
            weight: 0.0,
            ..death.sample()
        };
        assert_eq!(camera(Some(sample)), camera(None));
    }

    #[test]
    fn a_non_finite_weight_falls_back_to_the_tracked_camera() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7);
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
        let death = DeathCamera::begin(live_eye(), -1.6, 7);
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
        let death = DeathCamera::begin(live_eye(), -1.6, 7);
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
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7);
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
        let mut death = DeathCamera::begin(live_eye(), -1.6, 7);
        death.advance(FALL_SECONDS * 10.0);
        assert_eq!(death.sample().weight, 1.0);
    }

    #[test]
    fn the_fallen_eye_stays_clear_of_the_near_plane() {
        for seed in 0..64 {
            let death = DeathCamera::begin(live_eye(), -1.6, seed);
            assert!(death.sample().eye.position.y > -1.6 + 0.1);
        }
    }

    #[test]
    fn the_fallen_view_lies_on_its_side_looking_along_the_floor() {
        for seed in 0..64 {
            let death = DeathCamera::begin(live_eye(), -1.6, seed);
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
        let a = DeathCamera::begin(live_eye(), -1.6, 11);
        let b = DeathCamera::begin(live_eye(), -1.6, 11);
        assert_eq!(a, b);

        let directions: Vec<f32> = (0..32)
            .map(|seed| DeathCamera::begin(live_eye(), -1.6, seed).target.position.x)
            .collect();
        let spread = directions.iter().cloned().fold(f32::MIN, f32::max).max(0.0)
            - directions.iter().cloned().fold(f32::MAX, f32::min).min(0.0);
        assert!(spread > FALL_LATERAL, "fall directions barely vary");
    }

    #[test]
    fn a_degenerate_tracked_rotation_does_not_poison_the_blend() {
        let death = DeathCamera::begin(live_eye(), -1.6, 7);
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
