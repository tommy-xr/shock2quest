//! Head-anchored placement for the VR frontend panels.
//!
//! A menu glued to the gaze cannot be looked *at*: it moves exactly as fast as
//! the eye, so nothing on it can be inspected off-center and the world behind
//! it is permanently occluded. Real headsets place a frontend panel **once**,
//! world-lock it, and only re-place it when the player has clearly walked or
//! turned away and stayed away. That is what this module does:
//!
//! - **Place on entry** from the head pose, using **yaw only** - the panel is
//!   gravity-aligned and vertical even if the player enters the menu looking at
//!   the floor.
//! - **World-locked** afterward: turning the head moves the panel *in view*.
//! - **Lazy recenter**: sustained gaze/position divergence (not a glance)
//!   re-places it, eased rather than teleported.
//!
//! The math lives in free functions so it is unit-testable without a runtime;
//! [`FrontendPanelAnchor`] is the small amount of state a scene keeps.

use std::time::Duration;

use cgmath::{Deg, InnerSpace, Matrix3, Quaternion, Rad, Rotation, Vector3, vec3};

use crate::ui::{FRONTEND_PANEL_DISTANCE, FRONTEND_PANEL_SIZE, WorldPanel};

/// Gaze may drift this far off the panel before a recenter starts counting.
pub const RECENTER_YAW_DEGREES: f32 = 60.0;
/// ...or the head may move this far (world units) from where it placed it.
pub const RECENTER_DISTANCE: f32 = 1.0;
/// The divergence must be *sustained* this long, so a glance across the room
/// never drags the menu along.
pub const RECENTER_HOLD_SECONDS: f32 = 1.0;
/// How long the re-placement takes. A teleporting panel reads as a glitch;
/// this is short enough to feel responsive and long enough to be followed.
pub const RECENTER_EASE_SECONDS: f32 = 0.3;

/// Where a panel was placed: the head position it was hung off, and the
/// horizontal (yaw-only) direction it was hung along.
///
/// Two vectors rather than a `WorldPanel` because these are the quantities that
/// interpolate meaningfully during a recenter - and because a placement is
/// gravity-aligned by construction, so there is no roll or pitch to carry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlacement {
    /// The head position at placement time, in pawn space.
    pub head_position: Vector3<f32>,
    /// Unit, horizontal (y == 0): the direction the panel hangs along.
    pub forward: Vector3<f32>,
}

/// The head's facing flattened onto the horizontal plane.
///
/// Yaw only, so a panel built from it is vertical however the head is pitched
/// or rolled. Two degenerate inputs are handled:
///
/// - the **zero quaternion**, which is what a runtime reports for an untracked
///   pose (rotating by it silently returns the input vector), is treated as
///   identity;
/// - a **vertical** forward (looking straight down or up), where the horizontal
///   projection vanishes. There the head's own **up** axis is used instead:
///   looking at the floor, the top of your head points where you are facing, so
///   the panel spawns ahead of the player rather than at an arbitrary yaw.
pub fn horizontal_forward(head_rotation: Quaternion<f32>) -> Vector3<f32> {
    let rotation = if head_rotation.magnitude2() < 1e-6 {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    } else {
        head_rotation.normalize()
    };

    let flatten = |v: Vector3<f32>| {
        let flat = vec3(v.x, 0.0, v.z);
        (flat.magnitude2() > 1e-6).then(|| flat.normalize())
    };

    flatten(rotation.rotate_vector(vec3(0.0, 0.0, -1.0)))
        // Looking straight down/up: the head's up axis carries the yaw.
        .or_else(|| flatten(rotation.rotate_vector(vec3(0.0, 1.0, 0.0))))
        // Unreachable for a real rotation (forward and up cannot both be
        // vertical), but a defined answer beats a NaN basis.
        .unwrap_or_else(|| vec3(0.0, 0.0, -1.0))
}

impl PanelPlacement {
    /// Place a panel from a head pose: at the head, along its yaw.
    pub fn from_head(head_position: Vector3<f32>, head_rotation: Quaternion<f32>) -> Self {
        Self {
            head_position,
            forward: horizontal_forward(head_rotation),
        }
    }

    /// The panel this placement describes.
    ///
    /// Same distance, size and honest basis as the gaze-glued
    /// [`crate::ui::frontend_panel`] - local +x the viewer's right, +y up, +Z
    /// pointing back at the viewer - but built from a stored placement instead
    /// of the live head, and gravity-aligned (+y is exactly world up).
    pub fn panel(&self) -> WorldPanel {
        let center = self.head_position + self.forward * FRONTEND_PANEL_DISTANCE;
        // Panel -> head, horizontal, so the panel faces the player squarely.
        let look_dir = -self.forward;
        let up = vec3(0.0, 1.0, 0.0);
        // The viewer looks along -look_dir, so their right is up x look_dir.
        let right = up.cross(look_dir).normalize();
        WorldPanel {
            center,
            rotation: Quaternion::from(Matrix3::from_cols(right, up, look_dir)),
            size: FRONTEND_PANEL_SIZE,
        }
    }

    /// Signed-magnitude angle between this placement's yaw and a head facing.
    pub fn yaw_offset_degrees(&self, head_rotation: Quaternion<f32>) -> f32 {
        let gaze = horizontal_forward(head_rotation);
        let dot = self.forward.dot(gaze).clamp(-1.0, 1.0);
        Deg::from(Rad(dot.acos())).0
    }

    /// Has the player turned or moved far enough that this placement is stale?
    ///
    /// Instantaneously stale, that is - the anchor still requires it to hold
    /// for [`RECENTER_HOLD_SECONDS`] before acting.
    pub fn is_stale(&self, head_position: Vector3<f32>, head_rotation: Quaternion<f32>) -> bool {
        self.yaw_offset_degrees(head_rotation) > RECENTER_YAW_DEGREES
            || (head_position - self.head_position).magnitude() > RECENTER_DISTANCE
    }
}

/// Interpolate between two placements. `t` is clamped to `[0, 1]`, so the
/// endpoints are exactly `from` and `to`.
pub fn lerp_placement(from: PanelPlacement, to: PanelPlacement, t: f32) -> PanelPlacement {
    let t = t.clamp(0.0, 1.0);
    // Smoothstep: no velocity discontinuity at either end, which is what makes
    // the re-placement read as a move rather than a jump.
    let s = t * t * (3.0 - 2.0 * t);
    let head_position = from.head_position + (to.head_position - from.head_position) * s;
    let blended = from.forward + (to.forward - from.forward) * s;
    let forward = if blended.magnitude2() < 1e-6 {
        // Exactly-opposed placements have no shortest arc; commit to the
        // target rather than producing a zero direction.
        to.forward
    } else {
        let flat = vec3(blended.x, 0.0, blended.z);
        if flat.magnitude2() < 1e-6 {
            to.forward
        } else {
            flat.normalize()
        }
    };
    PanelPlacement {
        head_position,
        forward,
    }
}

/// An in-progress eased recenter.
#[derive(Debug, Clone, Copy)]
struct Ease {
    from: PanelPlacement,
    to: PanelPlacement,
    /// Seconds elapsed into [`RECENTER_EASE_SECONDS`].
    elapsed: f32,
}

/// The stateful half: place on entry, world-lock, lazily recenter.
///
/// A frontend scene owns one, feeds it the head pose every update, and both
/// hit-tests and renders against the panel it returns - so the ray and the art
/// can never disagree about where the menu is.
#[derive(Debug, Clone, Default)]
pub struct FrontendPanelAnchor {
    placement: Option<PanelPlacement>,
    ease: Option<Ease>,
    /// How long the placement has been continuously stale.
    stale_for: f32,
}

impl FrontendPanelAnchor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the anchor for a frame and return where the panel now is.
    ///
    /// The first call places the panel; later calls leave it alone unless a
    /// recenter is due or in progress.
    pub fn update(
        &mut self,
        head_position: Vector3<f32>,
        head_rotation: Quaternion<f32>,
        dt: Duration,
    ) -> WorldPanel {
        let dt = dt.as_secs_f32();
        let Some(current) = self.placement else {
            let placed = PanelPlacement::from_head(head_position, head_rotation);
            self.placement = Some(placed);
            return placed.panel();
        };

        if let Some(mut ease) = self.ease {
            ease.elapsed += dt;
            let t = ease.elapsed / RECENTER_EASE_SECONDS;
            let placement = lerp_placement(ease.from, ease.to, t);
            self.placement = Some(placement);
            self.ease = (t < 1.0).then_some(ease);
            return placement.panel();
        }

        if current.is_stale(head_position, head_rotation) {
            self.stale_for += dt;
            if self.stale_for >= RECENTER_HOLD_SECONDS {
                self.stale_for = 0.0;
                self.ease = Some(Ease {
                    from: current,
                    to: PanelPlacement::from_head(head_position, head_rotation),
                    elapsed: 0.0,
                });
            }
        } else {
            // Hysteresis: the clock only counts *sustained* divergence, so a
            // glance away and back leaves the panel where it was.
            self.stale_for = 0.0;
        }

        current.panel()
    }

    /// Where the panel is, as of the last [`update`](Self::update). `None`
    /// before the scene's first update.
    pub fn placement(&self) -> Option<PanelPlacement> {
        self.placement
    }

    /// The current panel, or a panel placed from a default head pose if the
    /// scene has not updated yet (render can run before update on the first
    /// frame of a scene swap).
    pub fn panel(&self) -> WorldPanel {
        self.placement
            .unwrap_or_else(|| {
                PanelPlacement::from_head(
                    vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                )
            })
            .panel()
    }

    /// Whether a recenter is currently animating (test/introspection hook).
    pub fn is_recentering(&self) -> bool {
        self.ease.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Rotation3, Zero};

    const FRAME: Duration = Duration::from_millis(1000 / 60);

    fn yaw(degrees: f32) -> Quaternion<f32> {
        Quaternion::from_angle_y(Deg(degrees))
    }

    fn eye() -> Vector3<f32> {
        vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0)
    }

    fn step(anchor: &mut FrontendPanelAnchor, pos: Vector3<f32>, rot: Quaternion<f32>, secs: f32) {
        let frames = (secs * 60.0).round() as u32;
        for _ in 0..frames {
            anchor.update(pos, rot, FRAME);
        }
    }

    #[test]
    fn a_pitched_head_still_places_an_upright_panel() {
        // Entering the menu while looking at the floor must not hang the panel
        // at the player's feet, face-down.
        let pitched = yaw(90.0) * Quaternion::from_angle_x(Deg(-70.0));
        let placement = PanelPlacement::from_head(eye(), pitched);
        let panel = placement.panel();

        assert!(
            (panel.center.y - eye().y).abs() < 1e-4,
            "panel should hang at eye level, not below it: {:?}",
            panel.center
        );
        let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
        assert!(
            (up - vec3(0.0, 1.0, 0.0)).magnitude() < 1e-4,
            "panel up must be gravity up, got {up:?}"
        );
        // Yaw preserved: the head faces +X at yaw 90 in this convention.
        let expected = yaw(90.0).rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!((placement.forward - expected).magnitude() < 1e-3);
    }

    #[test]
    fn looking_straight_down_keeps_the_heads_yaw() {
        // The forward axis is vertical here, so the projection is degenerate
        // and the fallback decides the answer. It must still be the yaw the
        // player is facing, not a fixed world axis.
        let straight_down = yaw(90.0) * Quaternion::from_angle_x(Deg(-90.0));
        let forward = horizontal_forward(straight_down);
        let expected = yaw(90.0).rotate_vector(vec3(0.0, 0.0, -1.0));
        assert!(
            (forward - expected).magnitude() < 1e-3,
            "expected {expected:?}, got {forward:?}"
        );
        assert!(forward.y.abs() < 1e-6, "forward must be horizontal");
    }

    #[test]
    fn an_untracked_head_is_treated_as_identity() {
        let forward = horizontal_forward(Quaternion::zero());
        assert!((forward - vec3(0.0, 0.0, -1.0)).magnitude() < 1e-6);
    }

    #[test]
    fn the_panel_is_world_locked_while_the_head_turns() {
        let mut anchor = FrontendPanelAnchor::new();
        let placed = anchor.update(eye(), yaw(0.0), FRAME);
        // A 30 degree turn, held for well past the hold time: inside the
        // threshold, so the panel must not move at all.
        step(&mut anchor, eye(), yaw(30.0), 3.0);
        let after = anchor.panel();
        assert!((after.center - placed.center).magnitude() < 1e-5);
        assert!(!anchor.is_recentering());
    }

    #[test]
    fn a_brief_glance_past_the_threshold_does_not_recenter() {
        let mut anchor = FrontendPanelAnchor::new();
        let placed = anchor.update(eye(), yaw(0.0), FRAME);
        step(&mut anchor, eye(), yaw(120.0), 0.5); // glance away...
        step(&mut anchor, eye(), yaw(0.0), 0.5); // ...and back
        step(&mut anchor, eye(), yaw(120.0), 0.5); // and away again
        assert!(
            !anchor.is_recentering(),
            "the stale clock must reset on the way back"
        );
        assert!((anchor.panel().center - placed.center).magnitude() < 1e-5);
    }

    #[test]
    fn sustained_turning_away_recenters_into_view() {
        let mut anchor = FrontendPanelAnchor::new();
        anchor.update(eye(), yaw(0.0), FRAME);
        step(&mut anchor, eye(), yaw(120.0), 1.1);
        assert!(anchor.is_recentering(), "recenter should have started");

        // Mid-ease the panel is between the two placements, not at either.
        let mid = anchor.placement().unwrap();
        assert!(mid.yaw_offset_degrees(yaw(120.0)) > 1.0);

        step(&mut anchor, eye(), yaw(120.0), 0.4);
        assert!(!anchor.is_recentering(), "the ease should have finished");
        let settled = anchor.placement().unwrap();
        assert!(
            settled.yaw_offset_degrees(yaw(120.0)) < 1.0,
            "the panel should end up in front of the new gaze, off by {}",
            settled.yaw_offset_degrees(yaw(120.0))
        );
    }

    #[test]
    fn walking_away_recenters_even_facing_the_same_way() {
        let mut anchor = FrontendPanelAnchor::new();
        anchor.update(eye(), yaw(0.0), FRAME);
        let moved = eye() + vec3(3.0, 0.0, 0.0);
        step(&mut anchor, moved, yaw(0.0), 1.5);
        step(&mut anchor, moved, yaw(0.0), 0.4);
        let settled = anchor.placement().unwrap();
        assert!(
            (settled.head_position - moved).magnitude() < 1e-3,
            "the panel should follow the player to {moved:?}, got {:?}",
            settled.head_position
        );
    }

    #[test]
    fn a_small_step_does_not_recenter() {
        let mut anchor = FrontendPanelAnchor::new();
        anchor.update(eye(), yaw(0.0), FRAME);
        step(&mut anchor, eye() + vec3(0.5, 0.0, 0.0), yaw(0.0), 3.0);
        assert!(!anchor.is_recentering());
    }

    #[test]
    fn the_ease_hits_its_endpoints_exactly() {
        let a = PanelPlacement::from_head(vec3(0.0, 1.0, 0.0), yaw(0.0));
        let b = PanelPlacement::from_head(vec3(2.0, 1.0, 3.0), yaw(90.0));
        let same = |x: PanelPlacement, y: PanelPlacement| {
            (x.head_position - y.head_position).magnitude() < 1e-5
                && (x.forward - y.forward).magnitude() < 1e-5
        };
        assert!(same(lerp_placement(a, b, 0.0), a));
        assert!(same(lerp_placement(a, b, 1.0), b));
        // Clamped, not extrapolated.
        assert!(same(lerp_placement(a, b, -1.0), a));
        assert!(same(lerp_placement(a, b, 5.0), b));
        let mid = lerp_placement(a, b, 0.5);
        assert!(mid.forward.y.abs() < 1e-6, "the ease must stay horizontal");
        assert!((mid.forward.magnitude() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn an_exactly_opposed_ease_still_yields_a_direction() {
        let a = PanelPlacement::from_head(Vector3::zero(), yaw(0.0));
        let b = PanelPlacement::from_head(Vector3::zero(), yaw(180.0));
        let mid = lerp_placement(a, b, 0.5);
        assert!((mid.forward.magnitude() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_placed_panel_faces_the_head() {
        let placement = PanelPlacement::from_head(eye(), yaw(37.0));
        let panel = placement.panel();
        let to_head = (eye() - panel.center).normalize();
        assert!(
            (panel.normal() - to_head).magnitude() < 1e-4,
            "the panel normal must point back at the viewer"
        );
        assert!(
            ((panel.center - eye()).magnitude() - FRONTEND_PANEL_DISTANCE).abs() < 1e-4,
            "the panel must hang at the frontend distance"
        );
    }
}
