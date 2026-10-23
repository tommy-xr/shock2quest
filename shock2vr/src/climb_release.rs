//! Where letting go of every climbing hold puts the player (#2376).
//!
//! One read-only [`ReleasePreview`] per frame drives the floor footprint, the
//! held gloves' green/red, and what a voluntary final release does. While a
//! ladder or grace hold is held, the tracked head can pick a nearby walkable
//! landing (an *assisted* release: a short collision-checked glide there);
//! otherwise, or when the gaze finds nothing valid, the preview is the
//! *natural* release - where the plain drop or throw would land.
//!
//! The geometry queries live in `physics::climb_landing`; this module holds the
//! pure rules they share.

use cgmath::{InnerSpace, Quaternion, Rotation, Vector3, vec3};

use crate::{
    input_context::InputContext,
    physics::{PhysicsWorld, PlayerHandle},
    vr_climb::HandClimb,
};

/// Farthest an assisted landing may be from the body centre, horizontally
/// (world units; about 1.5 m). Wall grace grabs cover anything farther.
pub const GAZE_MAX_HORIZONTAL: f32 = 2.0;
/// Deepest an assisted landing may sit below the feet (about 3 m). Generous:
/// a lower landing is reached by the glide plus an ordinary fall, which keeps
/// its fall risk (and colour).
pub const GAZE_MAX_DOWN: f32 = 4.0;
/// Highest an assisted landing may sit above the feet (about 0.95 m): enough
/// to step off a ladder's top onto its deck, still short of the waist-high
/// lip a hand vault already climbs, so the assist is never a way to climb.
pub const GAZE_MAX_UP: f32 = 1.25;
/// How far along the head ray a landing surface may be seen.
pub const GAZE_MAX_RAY: f32 = 8.0;
/// A gaze landing stays chosen while the aim lands within this of it, so
/// tracking jitter cannot make the footprint hop between two insets.
pub const GAZE_HOLD_RADIUS: f32 = 0.75;
/// Snap rings around the aimed point, searched nearest first, for an inset the
/// body fits (world units).
pub const GAZE_SNAP_RINGS: [f32; 2] = [0.3, 0.6];
const GAZE_SNAP_DIRECTIONS: usize = 8;
/// A snapped floor must be the aimed surface, not a step beside it.
pub const GAZE_SNAP_MAX_STEP: f32 = 0.15;

/// The gaze cone as (right, down) offsets in degrees, centre first. Wider
/// downward: a landing is seen at a grazing angle, so a small head pitch error
/// moves the aim a long way along the floor.
const GAZE_CONE_DEGREES: [(f32, f32); 9] = [
    (0.0, 0.0),
    (0.0, 4.0),
    (0.0, -4.0),
    (4.0, 0.0),
    (-4.0, 0.0),
    (0.0, 8.0),
    (4.0, 4.0),
    (-4.0, 4.0),
    (0.0, 12.0),
];

/// Where a release would leave the body: the resting body centre and the
/// floor under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub center: Vector3<f32>,
    pub floor_y: f32,
    /// The floor's surface normal there, for laying the footprint on it.
    pub normal: Vector3<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseMode {
    /// A gaze-selected landing a release glides to.
    Assisted,
    /// Where the plain drop or throw lands.
    Natural,
}

impl ReleaseMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Assisted => "assisted",
            Self::Natural => "natural",
        }
    }
}

/// Why the gaze offered no assisted landing this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GazeRejection {
    /// Head untracked, or no ladder/grace hold to exit from.
    Inactive,
    NoHit,
    NotWalkable,
    TooFar,
    TooHigh,
    TooLow,
    /// The standing body does not fit there (walls, low ceiling).
    NoFit,
    Unsupported,
    /// The glide there is obstructed.
    Blocked,
}

impl GazeRejection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::NoHit => "no_hit",
            Self::NotWalkable => "not_walkable",
            Self::TooFar => "too_far",
            Self::TooHigh => "too_high",
            Self::TooLow => "too_low",
            Self::NoFit => "no_fit",
            Self::Unsupported => "unsupported",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReleaseOutcome {
    pub mode: ReleaseMode,
    pub landing: Landing,
    /// Feet height now minus the landing floor (negative: a step up).
    pub drop: f32,
    /// A validated landing within the acceptable drop.
    pub safe: bool,
}

impl ReleaseOutcome {
    pub fn new(mode: ReleaseMode, landing: Landing, feet_y: f32, safe_drop: f32) -> Self {
        let drop = feet_y - landing.floor_y;
        Self {
            mode,
            landing,
            drop,
            safe: drop <= safe_drop,
        }
    }
}

/// The resolved release for one frame. No `outcome` means nothing can be
/// predicted reliably: no footprint, and the held gloves read unsafe.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReleasePreview {
    pub outcome: Option<ReleaseOutcome>,
    /// The aimed surface point behind the current gaze verdict.
    pub gaze_candidate: Option<Vector3<f32>>,
    pub gaze_rejection: Option<GazeRejection>,
}

impl ReleasePreview {
    /// The assisted landing a release would commit, if one is shown.
    pub fn assisted(&self) -> Option<Landing> {
        self.outcome
            .filter(|outcome| outcome.mode == ReleaseMode::Assisted)
            .map(|outcome| outcome.landing)
    }

    /// Whether the held gloves read safe.
    pub fn safe(&self) -> bool {
        self.outcome.is_some_and(|outcome| outcome.safe)
    }

    /// An automatic ledge vault onto `grip_point` would take the body somewhere
    /// other than the assisted landing on show: the release owns the exit.
    /// A vault onto the shown deck is the same exit and keeps working.
    pub fn blocks_vault(&self, grip_point: Vector3<f32>) -> bool {
        self.assisted().is_some_and(|landing| {
            let across = vec3(
                landing.center.x - grip_point.x,
                0.0,
                landing.center.z - grip_point.z,
            );
            across.magnitude() > VAULT_SAME_EXIT_RADIUS
                || (landing.floor_y - grip_point.y).abs() > GAZE_SNAP_MAX_STEP * 2.0
        })
    }
}

/// A vault's landing is an inset of up to 0.8 past the held lip; within this
/// of the lip, the shown landing is that same deck.
const VAULT_SAME_EXIT_RADIUS: f32 = 1.2;

/// The gaze rays for a head looking along `forward`, centre first.
/// Empty for a degenerate direction.
pub fn gaze_rays(forward: Vector3<f32>) -> Vec<Vector3<f32>> {
    if forward.magnitude2() < 1e-6 {
        return Vec::new();
    }
    let forward = forward.normalize();
    let side = forward.cross(Vector3::unit_y());
    // Looking straight up or down: any perpendicular will do.
    let right = if side.magnitude2() < 1e-6 {
        Vector3::unit_x()
    } else {
        side.normalize()
    };
    let up = right.cross(forward);
    GAZE_CONE_DEGREES
        .iter()
        .map(|(yaw, down)| {
            (forward + right * yaw.to_radians().tan() - up * down.to_radians().tan()).normalize()
        })
        .collect()
}

/// Horizontal offsets around an aimed point to try a landing at, nearest first.
pub fn snap_offsets() -> impl Iterator<Item = Vector3<f32>> {
    std::iter::once(vec3(0.0, 0.0, 0.0)).chain(GAZE_SNAP_RINGS.into_iter().flat_map(|radius| {
        (0..GAZE_SNAP_DIRECTIONS).map(move |i| {
            let angle = i as f32 / GAZE_SNAP_DIRECTIONS as f32 * std::f32::consts::TAU;
            vec3(angle.cos() * radius, 0.0, angle.sin() * radius)
        })
    }))
}

/// Whether a floor point is in assist range of a body at `body` with feet at
/// `feet_y`: measured from the body and the feet, never the head or the hands.
pub fn range_rejection(
    body: Vector3<f32>,
    feet_y: f32,
    floor: Vector3<f32>,
) -> Option<GazeRejection> {
    let rise = floor.y - feet_y;
    if vec3(floor.x - body.x, 0.0, floor.z - body.z).magnitude() > GAZE_MAX_HORIZONTAL {
        Some(GazeRejection::TooFar)
    } else if rise > GAZE_MAX_UP {
        Some(GazeRejection::TooHigh)
    } else if rise < -GAZE_MAX_DOWN {
        Some(GazeRejection::TooLow)
    } else {
        None
    }
}

/// Whether an aimed floor point keeps `held` chosen (see [`GAZE_HOLD_RADIUS`]).
pub fn keeps_held(held: Landing, aimed: Vector3<f32>) -> bool {
    vec3(aimed.x - held.center.x, 0.0, aimed.z - held.center.z).magnitude() <= GAZE_HOLD_RADIUS
        && (aimed.y - held.floor_y).abs() <= GAZE_SNAP_MAX_STEP
}

/// Resolve this frame's release preview for a VR body hanging from `climb`'s
/// holds (none while nothing is held or a scripted move owns the body).
/// `previous` supplies the gaze landing to hold through jitter; `safe_drop`
/// is the acceptable drop in world units.
pub fn resolve(
    physics: &PhysicsWorld,
    player: &PlayerHandle,
    climb: &HandClimb,
    input: &InputContext,
    pawn_rotation: Quaternion<f32>,
    previous: &ReleasePreview,
    safe_drop: f32,
) -> ReleasePreview {
    if climb.grips().next().is_none() || player.is_topping_out() {
        return ReleasePreview::default();
    }
    let center = physics.get_player_next_translation(player);
    // From the standing capsule while balled up on a ledge, like the grip rule.
    let feet_y = center.y
        - crate::physics::player_center_above_floor(
            player.is_crouched() && !player.is_hanging_crouched(),
        );
    // Only a ladder exit is assisted (through its grace grabs and deck
    // transfers): an ordinary ledge pull keeps its vault.
    let exits_a_ladder = climb.on_ladder_departure();
    // The same tracking a release needs to commit: never show an assisted
    // landing the release would then refuse.
    let tracked = input
        .pose_tracking
        .is_none_or(|tracking| tracking.head && tracking.hands == [true; 2]);
    let head = crate::util::tracked_rotation(input.head.rotation).filter(|_| tracked);
    let gaze = head.filter(|_| exits_a_ladder).map(|head| {
        physics.gaze_landing(
            player,
            center + pawn_rotation.rotate_vector(input.head.position),
            &gaze_rays((pawn_rotation * head).rotate_vector(vec3(0.0, 0.0, -1.0))),
            feet_y,
            previous.assisted(),
        )
    });
    // Without a gaze landing, a floor a hand holds is where letting go puts
    // the body (the anchor hand's first): held, so reachable without aiming.
    let assisted = gaze
        .as_ref()
        .and_then(|gaze| gaze.landing.ok())
        .or_else(|| {
            (exits_a_ladder && tracked)
                .then(|| {
                    let anchor_hand = climb.anchor();
                    climb
                        .grips()
                        .filter(|(hand, _)| Some(*hand) == anchor_hand)
                        .chain(climb.grips().filter(|(hand, _)| Some(*hand) != anchor_hand))
                        .map(|(_, anchor)| anchor)
                        .find_map(|anchor| {
                            physics.held_floor_landing(player, &anchor.grip, previous.assisted())
                        })
                })
                .flatten()
        });
    let outcome = match assisted {
        Some(landing) => Some(ReleaseOutcome::new(
            ReleaseMode::Assisted,
            landing,
            feet_y,
            safe_drop,
        )),
        None => physics
            .natural_landing(
                player,
                climb.predicted_release_velocity(physics.player_step_dt()),
            )
            .map(|landing| ReleaseOutcome::new(ReleaseMode::Natural, landing, feet_y, safe_drop)),
    };
    ReleasePreview {
        outcome,
        gaze_candidate: gaze.as_ref().and_then(|gaze| gaze.candidate),
        gaze_rejection: match gaze {
            Some(gaze) => gaze.landing.err(),
            None => Some(GazeRejection::Inactive),
        },
    }
}

/// Seconds an exit teleport takes to fade to black, and back in after moving.
pub const EXIT_FADE_OUT: f32 = 0.1;
pub const EXIT_FADE_IN: f32 = 0.15;

/// A committed assisted release taken as a teleport: fade to black, move the
/// body once at black, fade back in. The hands take no hold until it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitTeleport {
    pub outcome: ReleaseOutcome,
    elapsed: f32,
    moved: bool,
}

impl ExitTeleport {
    pub fn new(outcome: ReleaseOutcome) -> Self {
        Self {
            outcome,
            elapsed: 0.0,
            moved: false,
        }
    }

    /// Advance by `dt`; `true` exactly once, when the view is black and the
    /// body should move. That frame renders fully black whatever `dt` was;
    /// the fade-in starts on the next.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        let move_now = !self.moved && self.elapsed >= EXIT_FADE_OUT - 1e-4;
        if move_now {
            self.moved = true;
            self.elapsed = EXIT_FADE_OUT;
        }
        move_now
    }

    /// Whether the body has been moved (the view is fading back in).
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// Black cover opacity, 0..=1.
    pub fn alpha(&self) -> f32 {
        if !self.moved {
            (self.elapsed / EXIT_FADE_OUT).clamp(0.0, 1.0)
        } else {
            (1.0 - (self.elapsed - EXIT_FADE_OUT) / EXIT_FADE_IN).clamp(0.0, 1.0)
        }
    }

    pub fn done(&self) -> bool {
        self.moved && self.alpha() <= 0.0
    }
}

/// How long a fresh hold flashes cyan before showing the release's colour.
pub const HOLD_PULSE_SECONDS: f32 = 0.25;

/// A held glove's light: the catch flash, then green for a safe release and
/// red for anything else (a long fall, or no landing known).
pub fn held_glove_light(held_seconds: f32, safe: bool) -> crate::HandLight {
    if held_seconds < HOLD_PULSE_SECONDS {
        crate::HandLight::ClimbHeld
    } else if safe {
        crate::HandLight::Green
    } else {
        crate::HandLight::Red
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landing(x: f32, floor_y: f32, z: f32) -> Landing {
        Landing {
            center: vec3(x, floor_y + 1.24, z),
            floor_y,
            normal: Vector3::unit_y(),
        }
    }

    #[test]
    fn the_cone_is_centred_on_the_head_and_wider_downward() {
        let rays = gaze_rays(vec3(0.0, 0.0, -1.0));
        assert_eq!(rays[0], vec3(0.0, 0.0, -1.0));
        let max_down = rays.iter().map(|r| -r.y).fold(f32::MIN, f32::max);
        let max_up = rays.iter().map(|r| r.y).fold(f32::MIN, f32::max);
        let max_side = rays.iter().map(|r| r.x.abs()).fold(f32::MIN, f32::max);
        assert!(max_down > max_up && max_down > max_side);
        assert!(max_side < 0.1, "narrow: {max_side}");
        // Straight down still yields rays; a zero direction yields none.
        assert_eq!(gaze_rays(vec3(0.0, -1.0, 0.0)).len(), 9);
        assert!(gaze_rays(vec3(0.0, 0.0, 0.0)).is_empty());
    }

    #[test]
    fn range_is_from_body_and_feet_and_more_generous_downward() {
        let body = vec3(0.0, 5.0, 0.0);
        let feet = 3.8;
        assert_eq!(range_rejection(body, feet, vec3(1.9, feet, 0.0)), None);
        assert_eq!(
            range_rejection(body, feet, vec3(2.1, feet, 0.0)),
            Some(GazeRejection::TooFar)
        );
        assert_eq!(
            range_rejection(body, feet, vec3(0.5, feet + 1.2, 0.0)),
            None
        );
        assert_eq!(
            range_rejection(body, feet, vec3(0.5, feet + 1.3, 0.0)),
            Some(GazeRejection::TooHigh)
        );
        assert_eq!(
            range_rejection(body, feet, vec3(0.5, feet - 3.9, 0.0)),
            None
        );
        assert_eq!(
            range_rejection(body, feet, vec3(0.5, feet - 4.1, 0.0)),
            Some(GazeRejection::TooLow)
        );
    }

    #[test]
    fn snapping_starts_at_the_aim_and_stays_bounded() {
        let offsets: Vec<_> = snap_offsets().collect();
        assert_eq!(offsets[0], vec3(0.0, 0.0, 0.0));
        let mut last = 0.0;
        for offset in &offsets {
            assert!(offset.magnitude() + 1e-5 >= last, "nearest first");
            last = offset.magnitude();
        }
        assert!(last <= 0.6 + 1e-5);
    }

    #[test]
    fn small_aim_jitter_keeps_the_held_landing_but_a_new_surface_does_not() {
        let held = landing(1.0, 4.0, 0.0);
        assert!(keeps_held(held, vec3(1.5, 4.0, 0.3)));
        assert!(!keeps_held(held, vec3(2.0, 4.0, 0.0)), "aimed away");
        assert!(!keeps_held(held, vec3(1.0, 4.5, 0.0)), "another level");
    }

    #[test]
    fn a_held_glove_flashes_on_the_catch_then_shows_the_release() {
        use crate::HandLight;
        assert_eq!(held_glove_light(0.0, true), HandLight::ClimbHeld);
        assert_eq!(held_glove_light(0.3, true), HandLight::Green);
        assert_eq!(held_glove_light(0.3, false), HandLight::Red);
    }

    #[test]
    fn an_exit_teleport_fades_out_moves_once_at_black_then_fades_in() {
        let outcome = ReleaseOutcome::new(ReleaseMode::Assisted, landing(0.0, 4.0, 0.0), 5.0, 1.3);
        let mut exit = ExitTeleport::new(outcome);
        assert_eq!(exit.alpha(), 0.0);
        let dt = 1.0 / 60.0;
        let mut moves = 0;
        let mut frames = 0;
        let mut peak = 0.0f32;
        while !exit.done() {
            if exit.advance(dt) {
                moves += 1;
                assert!(exit.alpha() > 0.99, "moves at black, not {}", exit.alpha());
            }
            peak = peak.max(exit.alpha());
            frames += 1;
            assert!(frames < 60, "ends within a second");
        }
        assert_eq!(moves, 1);
        assert!(peak > 0.99);
        // About 0.1 s out and 0.15 s in.
        assert!((14..=17).contains(&frames), "{frames}");
        assert_eq!(exit.alpha(), 0.0);

        // A long or uneven frame still moves under full black.
        for dt in [0.3, 1.0 / 72.0] {
            let mut exit = ExitTeleport::new(outcome);
            while !exit.advance(dt) {}
            assert_eq!(exit.alpha(), 1.0, "black on the move frame at dt {dt}");
        }
    }

    #[test]
    fn safety_is_the_drop_against_the_threshold() {
        let feet = 5.0;
        let safe = ReleaseOutcome::new(ReleaseMode::Natural, landing(0.0, 4.0, 0.0), feet, 1.3);
        assert!(safe.safe);
        assert!((safe.drop - 1.0).abs() < 1e-5);
        let long = ReleaseOutcome::new(ReleaseMode::Natural, landing(0.0, 0.0, 0.0), feet, 1.3);
        assert!(!long.safe);
        let up = ReleaseOutcome::new(ReleaseMode::Assisted, landing(0.0, 5.5, 0.0), feet, 1.3);
        assert!(up.safe && up.drop < 0.0);
        assert!(
            !ReleasePreview::default().safe(),
            "no prediction is not safe"
        );
    }

    #[test]
    fn a_shown_assisted_exit_blocks_only_a_vault_somewhere_else() {
        let lip = vec3(-7.0, 6.0, 0.0);
        let preview = |mode, landing| ReleasePreview {
            outcome: Some(ReleaseOutcome::new(mode, landing, 5.0, 1.3)),
            ..Default::default()
        };
        // The deck past the held lip: same exit, the vault keeps working.
        assert!(!preview(ReleaseMode::Assisted, landing(-7.6, 6.0, 0.0)).blocks_vault(lip));
        // A side platform: the vault must not take over before release.
        assert!(preview(ReleaseMode::Assisted, landing(-6.0, 4.0, 2.0)).blocks_vault(lip));
        // A natural preview never blocks an ordinary vault.
        assert!(!preview(ReleaseMode::Natural, landing(-6.0, 0.0, 0.0)).blocks_vault(lip));
        assert!(!ReleasePreview::default().blocks_vault(lip));
    }
}
