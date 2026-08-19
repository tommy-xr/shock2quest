//! Feedback for the player taking damage: a hurt grunt and a brief red rim
//! tint around the view.
//!
//! Before this, damage to the player was completely silent and invisible -
//! hit points ticked down in the HUD's bio meter and nothing else happened, so
//! in VR (which has no bio meter in the corner of the eye) a hybrid could beat
//! the player to death without a single cue. Issue: "need some feedback when
//! hit".
//!
//! # Where the pieces live
//!
//! The *trigger* is one hook in `mission_core`'s `AdjustHitPoints` applier:
//! every hit-point change in the game flows through it, so a single site
//! covers melee, projectiles, psi, and anything added later, and it sees the
//! damage that was actually *applied* rather than what a weapon asked for.
//!
//! The *presentation* is [`HitFeedback`], owned by `Game` next to the pause
//! menu, because both are view-locked layers over whatever scene is running
//! and both need the tracked head pose that `Game` already keeps.
//!
//! # Comfort
//!
//! A full-screen red flash is the obvious implementation and the wrong one in
//! a headset: it fills the fovea, it is exactly the stimulus that reads as
//! "something is wrong with my eyes" rather than "I was hit", and at
//! HUD-of-a-shooter intensities it is genuinely unpleasant to wear. So this is
//! a *rim* tint - clear through the middle of the field, ramping to red toward
//! the periphery, where motion and colour changes are noticed without being
//! stared at. It is brief (well under a second), and both how strong it gets
//! and how long it lasts scale with the size of the hit, so chip damage is a
//! flicker and a shotgun blast is unmistakable.

use cgmath::{Deg, Matrix4, Quaternion, Vector3, vec3};
use engine::scene::SceneObject;

/// Damage that produces a full-strength tint. The player's authored pool is 30
/// hit points (`The Player`, `P$HitPoints`), so this is "a bit over a third of
/// your health at once" - a shotgun blast, not a hybrid pipe swing.
const DAMAGE_FOR_FULL_STRENGTH: f32 = 12.0;

/// Rim opacity at the peak of a full-strength hit. Deliberately short of
/// opaque: this layer sits over the whole periphery, and the player still has
/// to fight through it.
const MAX_RIM_OPACITY: f32 = 0.65;

/// How long the tint lasts for the smallest and for a full-strength hit.
/// Both are brief; the difference is what makes a big hit read as bigger.
const MIN_DURATION_SECS: f32 = 0.35;
const MAX_DURATION_SECS: f32 = 0.85;

/// Arterial red. Not orange (reads as fire/heat) and not pink (reads as UI).
const TINT_COLOR: Vector3<f32> = Vector3 {
    x: 0.62,
    y: 0.02,
    z: 0.02,
};

/// How far in front of the eyes the layer hangs, in metres. Near enough that
/// nothing in the world can be between it and the eye in practice; the layer
/// clears depth anyway, so this only has to stay outside the near plane.
const LAYER_DISTANCE: f32 = 0.6;

/// Half-edge of the layer quad as a multiple of its distance, so coverage is
/// an angle and not a size: `atan(5.0)` = 78.7 degrees off-axis in every
/// direction, against a Quest's ~55 degree half-field. This is the same
/// argument (and the same number) as the pause menu's comfort dim - see
/// `pause_menu::WORLD_DIM_EXTENT_RATIO`.
const LAYER_EXTENT_RATIO: f32 = 5.0;

/// Where the tint starts and where it reaches full strength, as half-angles
/// off the view axis. Everything inside [`CLEAR_HALF_ANGLE_DEG`] is untouched
/// - that is the part of the field the player is actually aiming and reading
/// with. These are shared by both presentations: flat's narrower field of view
/// simply shows more of the clear centre and less of the rim, which is the
/// correct behaviour for an effect defined in the player's field of view
/// rather than in pixels.
const CLEAR_HALF_ANGLE_DEG: f32 = 26.0;
const FULL_HALF_ANGLE_DEG: f32 = 52.0;

/// Convert a half-angle off the view axis into the shader's radius units,
/// where 1.0 is the middle of the quad's edge.
fn radius_at_half_angle(degrees: f32) -> f32 {
    degrees.to_radians().tan() / LAYER_EXTENT_RATIO
}

/// Peak rim opacity for a hit of `damage` hit points.
pub fn peak_opacity(damage: f32) -> f32 {
    let scale = (damage / DAMAGE_FOR_FULL_STRENGTH).clamp(0.0, 1.0);
    // Square-root, so a 1-point graze is still visible (0.29 of full) instead
    // of being a rounding error, while the top of the range stays reserved for
    // hits that really are big.
    MAX_RIM_OPACITY * scale.sqrt()
}

/// How long the tint lasts for a hit of `damage` hit points.
pub fn duration_secs(damage: f32) -> f32 {
    let scale = (damage / DAMAGE_FOR_FULL_STRENGTH).clamp(0.0, 1.0);
    MIN_DURATION_SECS + (MAX_DURATION_SECS - MIN_DURATION_SECS) * scale
}

/// The schema the player grunts with, by how hard they were hit.
///
/// These are the original game's own player-voice schemas (`SPEECH_PLAYER` ->
/// `smallouch` / `medouch` / `bigouch`); they carry no environmental-sound
/// tags, so they are addressed by name the way the retail scripts did rather
/// than through an `EnvSoundQuery` (verified: `dark_query sound
/// +creaturetype:player` and `+event:hit` both match nothing). The thresholds
/// themselves are a port choice - the retail cutoffs were inside compiled
/// script - chosen against the player's 30-point pool.
pub fn hurt_schema(damage: f32) -> &'static str {
    if damage < 5.0 {
        "smallouch"
    } else if damage < 12.0 {
        "medouch"
    } else {
        "bigouch"
    }
}

/// The decaying state of the hit tint. Pure: it is advanced with a delta time
/// and asked for its current strength, and knows nothing about rendering.
#[derive(Debug, Default)]
pub struct HitFeedback {
    /// Strength the current hit started at.
    peak: f32,
    /// Total and remaining lifetime of the current hit, in seconds.
    duration: f32,
    remaining: f32,
}

impl HitFeedback {
    pub fn new() -> HitFeedback {
        HitFeedback::default()
    }

    /// Record a hit of `damage` applied hit points.
    ///
    /// A second hit while one is still showing restarts the decay, and never
    /// makes the tint jump *down*: taking a graze mid-fade from a shotgun
    /// blast must not look like the blast stopped hurting. So the new peak is
    /// the stronger of the incoming hit and whatever is on screen right now.
    pub fn trigger(&mut self, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        self.peak = peak_opacity(damage).max(self.intensity());
        self.duration = duration_secs(damage);
        self.remaining = self.duration;
    }

    /// Advance the decay by `delta_time_secs`.
    pub fn update(&mut self, delta_time_secs: f32) {
        self.remaining = (self.remaining - delta_time_secs).max(0.0);
    }

    /// Forget any hit in flight - used when the campaign is replaced, so a
    /// tint cannot survive into a freshly loaded game.
    pub fn clear(&mut self) {
        self.remaining = 0.0;
        self.peak = 0.0;
    }

    /// Current rim opacity, 0 when nothing is showing. Linear decay: it
    /// reaches zero continuously, so the layer never pops off.
    pub fn intensity(&self) -> f32 {
        if self.duration <= 0.0 || self.remaining <= 0.0 {
            return 0.0;
        }
        self.peak * (self.remaining / self.duration)
    }

    /// The layer to draw this frame, if anything is showing.
    ///
    /// `eye_position` and `eye_forward` are in the same space the returned
    /// object is consumed in - `Game` builds them in pawn space and maps the
    /// result with its pawn-to-world transform, exactly as it does for the
    /// pause panel.
    pub fn render(
        &self,
        eye_position: Vector3<f32>,
        eye_forward: Vector3<f32>,
    ) -> Option<SceneObject> {
        let intensity = self.intensity();
        if intensity <= 0.0 {
            return None;
        }
        Some(hit_layer(eye_position, eye_forward, intensity))
    }
}

/// One view-locked, double-sided quad carrying the rim tint.
///
/// The geometry follows the pause menu's comfort dim (PR #1020) rather than
/// being a screen-space overlay, and for the reasons documented there: the two
/// VR hosts disagree about where `render_per_eye` objects land in the scene
/// list (the debug runtime appends, `oculus_runtime` prepends), so a
/// screen-space layer that has to sit *over the world* is drawn correctly on
/// exactly one of them. A world-space quad emitted from `render` is ordered by
/// us. For the same reason it does not opt into backface culling - the hosts
/// also disagree about winding, and a culled half was the original #1020 bug.
///
/// Because it is view-locked and defined by an angular ratio, the *same* code
/// serves flat and VR: the eye pose differs, nothing else does.
fn hit_layer(eye_position: Vector3<f32>, eye_forward: Vector3<f32>, intensity: f32) -> SceneObject {
    let extent = LAYER_DISTANCE * LAYER_EXTENT_RATIO;
    let mut object = SceneObject::new(
        engine::scene::vignette_material::create(
            TINT_COLOR,
            intensity,
            radius_at_half_angle(CLEAR_HALF_ANGLE_DEG),
            radius_at_half_angle(FULL_HALF_ANGLE_DEG),
        ),
        Box::new(engine::scene::quad::create()),
    );
    object.set_transform(
        Matrix4::from_translation(eye_position + eye_forward * LAYER_DISTANCE)
            // The quad's +Z faces the viewer once it is turned to look back
            // along the gaze.
            * Matrix4::from(crate::util::get_rotation_from_forward_vector(-eye_forward))
            * Matrix4::from_scale(extent * 2.0),
    );
    // Translucent: writing depth here would let the layer occlude anything
    // drawn after it in the same overlay group.
    object.set_depth_write(false);
    // The layer must cover the world regardless of what the player is standing
    // in front of, so it starts an overlay group (see `pause_menu`'s dim - the
    // renderer treats everything from the first `clear_depth` object onward as
    // a group drawn after the world's passes). `Game` emits it before the
    // pause menu's objects, so a hit that lands as the menu opens stays behind
    // the panel.
    object.set_clear_depth(true);
    object.set_debug_tag(Some(crate::util::render_source_tag(
        crate::util::render_source::HIT_FEEDBACK,
    )));
    object
}

/// The eye pose to hang the layer from, in pawn space.
///
/// An untracked head arrives as the ZERO quaternion, which cgmath's
/// `rotate_vector` silently returns *unrotated* - so it is treated as "no
/// pose" and the pawn's own default eye stands in, rather than as a valid
/// forward that would hang the layer off to one side (VR rule 7).
pub fn eye_pose(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    use cgmath::{InnerSpace, Rotation};
    if head_rotation.magnitude2() < 1e-6 {
        return (
            vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
            vec3(0.0, 0.0, -1.0),
        );
    }
    (
        head_position,
        head_rotation
            .normalize()
            .rotate_vector(vec3(0.0, 0.0, -1.0)),
    )
}

/// Only used to document the angular constants in tests.
#[allow(dead_code)]
fn half_angle_at_radius(radius: f32) -> Deg<f32> {
    Deg((radius * LAYER_EXTENT_RATIO).atan().to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Rotation3, Zero};

    #[test]
    fn nothing_shows_until_something_hits_the_player() {
        let feedback = HitFeedback::new();
        assert_eq!(feedback.intensity(), 0.0);
        assert!(
            feedback
                .render(Vector3::zero(), vec3(0.0, 0.0, -1.0))
                .is_none()
        );
    }

    #[test]
    fn a_hit_shows_and_then_decays_to_nothing() {
        let mut feedback = HitFeedback::new();
        feedback.trigger(6.0);
        assert!(feedback.intensity() > 0.0);
        assert!(
            feedback
                .render(Vector3::zero(), vec3(0.0, 0.0, -1.0))
                .is_some()
        );

        // Halfway through its life it is weaker but still showing.
        let duration = duration_secs(6.0);
        feedback.update(duration * 0.5);
        let midway = feedback.intensity();
        assert!(midway > 0.0 && midway < peak_opacity(6.0));

        // And it goes away on its own, without anything clearing it.
        feedback.update(duration);
        assert_eq!(feedback.intensity(), 0.0);
        assert!(
            feedback
                .render(Vector3::zero(), vec3(0.0, 0.0, -1.0))
                .is_none()
        );
    }

    #[test]
    fn a_bigger_hit_is_stronger_and_lasts_longer() {
        assert!(peak_opacity(12.0) > peak_opacity(2.0));
        assert!(duration_secs(12.0) > duration_secs(2.0));
        // ...and neither runs away past the tuned ceiling.
        assert_eq!(peak_opacity(500.0), MAX_RIM_OPACITY);
        assert_eq!(duration_secs(500.0), MAX_DURATION_SECS);
    }

    #[test]
    fn even_a_single_point_of_damage_is_visible() {
        // A linear scale would put a 1-point graze at 0.05 opacity, which is
        // indistinguishable from nothing on a headset panel.
        assert!(peak_opacity(1.0) > 0.15);
    }

    #[test]
    fn healing_and_zero_damage_show_nothing() {
        let mut feedback = HitFeedback::new();
        feedback.trigger(0.0);
        assert_eq!(feedback.intensity(), 0.0);
        feedback.trigger(-5.0);
        assert_eq!(feedback.intensity(), 0.0);
    }

    #[test]
    fn a_graze_mid_fade_never_makes_the_tint_jump_down() {
        let mut feedback = HitFeedback::new();
        feedback.trigger(12.0);
        feedback.update(duration_secs(12.0) * 0.25);
        let before = feedback.intensity();

        feedback.trigger(1.0);
        assert!(
            feedback.intensity() >= before,
            "a small hit during a big one's fade dropped the tint from {before} to {}",
            feedback.intensity()
        );
    }

    #[test]
    fn clearing_removes_a_tint_in_flight() {
        let mut feedback = HitFeedback::new();
        feedback.trigger(10.0);
        assert!(feedback.intensity() > 0.0);
        feedback.clear();
        assert_eq!(feedback.intensity(), 0.0);
    }

    #[test]
    fn the_hurt_grunt_gets_louder_with_the_hit() {
        assert_eq!(hurt_schema(1.0), "smallouch");
        assert_eq!(hurt_schema(8.0), "medouch");
        assert_eq!(hurt_schema(25.0), "bigouch");
    }

    /// The whole comfort argument is that the middle of the field is left
    /// alone. If the ramp ever started at the view axis this would be the
    /// full-screen flash the design rejects.
    #[test]
    fn the_middle_of_the_view_is_left_clear() {
        let inner = radius_at_half_angle(CLEAR_HALF_ANGLE_DEG);
        assert!(inner > 0.0, "the tint must not start on the view axis");
        assert!(
            half_angle_at_radius(inner) > Deg(20.0),
            "the clear centre must cover the part of the field the player reads with"
        );
        assert!(radius_at_half_angle(FULL_HALF_ANGLE_DEG) > inner);
    }

    /// #1020's bug in miniature: a layer that relies on the host agreeing
    /// about winding covers only part of the view on the Quest.
    #[test]
    fn the_layer_does_not_depend_on_backface_culling() {
        let object = hit_layer(Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        assert_eq!(object.backface_culling(), None);
    }

    #[test]
    fn the_layer_draws_translucent_and_over_the_world() {
        let object = hit_layer(Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        let transparency = object
            .effective_transparency()
            .expect("the tint must draw translucent, not opaque");
        assert!(transparency > 0.0 && transparency < 1.0);
        assert!(
            object.clear_depth,
            "a layer depth-tested against the world would only tint what is far away"
        );
        assert_eq!(
            object.debug_tag().and_then(|tag| tag.source.clone()),
            Some(crate::util::render_source::HIT_FEEDBACK.to_owned())
        );
    }

    #[test]
    fn the_layer_hangs_in_front_of_wherever_the_player_is_looking() {
        let eye = vec3(3.0, 1.5, -2.0);
        for forward in [
            vec3(0.0, 0.0, -1.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0).normalize(),
            vec3(-0.5, -0.5, 0.7).normalize(),
        ] {
            let object = hit_layer(eye, forward, 0.5);
            let centre = object.get_transform() * cgmath::vec4(0.0, 0.0, 0.0, 1.0);
            let offset = vec3(centre.x, centre.y, centre.z) - eye;
            assert!(
                (offset.normalize() - forward).magnitude() < 1e-3,
                "layer centre {offset:?} is not along the gaze {forward:?}"
            );
            assert!((offset.magnitude() - LAYER_DISTANCE).abs() < 1e-3);
        }
    }

    #[test]
    fn an_untracked_head_falls_back_to_the_pawn_eye_instead_of_a_zero_rotation() {
        let (position, forward) =
            eye_pose(vec3(9.0, 9.0, 9.0), Quaternion::new(0.0, 0.0, 0.0, 0.0));
        assert_eq!(
            position,
            vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0)
        );
        assert_eq!(forward, vec3(0.0, 0.0, -1.0));
    }

    #[test]
    fn a_tracked_head_is_used_as_given() {
        let head = vec3(0.1, 1.7, -0.2);
        let rotation = Quaternion::from_angle_y(Deg(90.0));
        let (position, forward) = eye_pose(head, rotation);
        assert_eq!(position, head);
        assert!((forward - vec3(-1.0, 0.0, 0.0)).magnitude() < 1e-5);
    }
}
