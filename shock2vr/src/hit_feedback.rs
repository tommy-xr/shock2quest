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

use cgmath::{Matrix4, Quaternion, Vector3, vec3};
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

/// Where the tint starts and where it reaches full strength, as fractions of
/// how far the presentation's field of view actually reaches. Everything
/// inside [`CLEAR_FIELD_FRACTION`] is untouched - that is the part of the
/// field the player aims and reads with - and the tint is at full strength by
/// the edge of the picture.
const CLEAR_FIELD_FRACTION: f32 = 0.5;
const FULL_FIELD_FRACTION: f32 = 1.0;

/// A sane fallback half-field, in degrees, for the frames before any
/// projection has been seen: the ~29 degrees both flat runtimes' 45-degree
/// 4:3 `perspective` gives.
pub const DEFAULT_HALF_FIELD_DEG: f32 = 29.0;

/// How far the picture reaches sideways from the view axis, read straight off
/// the projection the host is rendering with.
///
/// This is why there is **no per-presentation branch in this module**
/// (AGENTS.md section 3): a rim effect is only a rim effect relative to where
/// the picture ends, and flat and VR disagree about that by roughly a factor
/// of two - both flat runtimes build `perspective(Deg(45.0))` while a Quest
/// eye reaches about 55 degrees. Hardcoding one angle is what the first cut
/// did, and it made the tint invisible on flat, with the whole ramp past the
/// edge of the screen. Hardcoding *two* would then have been a lie in the
/// debug runtime, whose `--vr` mode renders with the flat 45-degree
/// projection. Asking the projection is right everywhere, including on a
/// headset whose per-eye frustum is asymmetric.
///
/// For any perspective or off-axis frustum, `m[0][0]` is `2n / (r - l)`, so
/// its reciprocal is the tangent of half the horizontal extent.
pub fn half_field_deg_from_projection(projection: Matrix4<f32>) -> f32 {
    let m00 = projection.x.x;
    if !m00.is_finite() || m00 <= 0.0 {
        return DEFAULT_HALF_FIELD_DEG;
    }
    (1.0 / m00).atan().to_degrees().clamp(5.0, 85.0)
}

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
/// These are the shipped gamesys's own player-damage schemas, resolving to the
/// `dmgenlo*` / `dmgenme*` / `dmgenhi*` samples in `res/snd/PDamage` - the
/// original game's player pain grunts. They are addressed **by name**, the way
/// the retail scripts did, because they carry no environmental-sound tags:
/// `dark_query sound +creaturetype:player` and `+event:hit` both match
/// nothing, so an `EnvSoundQuery` cannot reach them.
///
/// Two traps are recorded here so nobody re-walks them. First, the obvious
/// candidates - the `SPEECH_PLAYER` schemas `smallouch` / `medouch` /
/// `bigouch` - are **dead**: they resolve, but to `pdamlo*` / `pdammed*` /
/// `pdamhi*` samples that ship in no `.kpf`, so wiring them plays silence.
/// Second, `PDamage` also holds damage-*type* sets (`dam_bullet_*`,
/// `dam_elec_*`, `dam_rad_*`, ...); the port does not carry a damage type
/// through `AdjustHitPoints`, so the generic set is the honest choice, and
/// picking a type-specific one is the natural follow-up once it does.
///
/// The thresholds are a port choice - the retail cutoffs lived in compiled
/// script - chosen against the player's 30-point pool.
pub fn hurt_schema(damage: f32) -> &'static str {
    if damage < 5.0 {
        "dam_gen_lo"
    } else if damage < 12.0 {
        "dam_gen_med"
    } else {
        "dam_gen_hi"
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
    /// pause panel. `half_field_deg` comes from the host's own projection -
    /// see [`half_field_deg_from_projection`].
    pub fn render(
        &self,
        half_field_deg: f32,
        eye_position: Vector3<f32>,
        eye_forward: Vector3<f32>,
    ) -> Option<SceneObject> {
        let intensity = self.intensity();
        if intensity <= 0.0 {
            return None;
        }
        Some(hit_layer(
            half_field_deg,
            eye_position,
            eye_forward,
            intensity,
        ))
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
/// Because it is view-locked, the *same* code serves flat and VR: the eye pose
/// differs, and the ramp is mapped onto whatever field of view the host is
/// actually rendering (see [`half_field_deg_from_projection`]). Nothing else
/// does.
fn hit_layer(
    half_field: f32,
    eye_position: Vector3<f32>,
    eye_forward: Vector3<f32>,
    intensity: f32,
) -> SceneObject {
    let extent = LAYER_DISTANCE * LAYER_EXTENT_RATIO;
    let mut object = SceneObject::new(
        engine::scene::vignette_material::create(
            TINT_COLOR,
            intensity,
            radius_at_half_angle(half_field * CLEAR_FIELD_FRACTION),
            radius_at_half_angle(half_field * FULL_FIELD_FRACTION),
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
#[cfg(test)]
fn half_angle_at_radius(radius: f32) -> cgmath::Deg<f32> {
    cgmath::Deg((radius * LAYER_EXTENT_RATIO).atan().to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, InnerSpace, Rotation3, Zero};

    #[test]
    fn nothing_shows_until_something_hits_the_player() {
        let feedback = HitFeedback::new();
        assert_eq!(feedback.intensity(), 0.0);
        assert!(
            feedback
                .render(VR_HALF_FIELD, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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
                .render(VR_HALF_FIELD, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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
                .render(VR_HALF_FIELD, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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
        assert_eq!(hurt_schema(1.0), "dam_gen_lo");
        assert_eq!(hurt_schema(8.0), "dam_gen_med");
        assert_eq!(hurt_schema(25.0), "dam_gen_hi");
    }

    /// A Quest eye reaches about this far off-axis; used to stand in for a
    /// headset in these tests.
    const VR_HALF_FIELD: f32 = 55.0;
    /// What both flat runtimes' `perspective(Deg(45.0))` on 4:3 gives.
    const FLAT_HALF_FIELD: f32 = 29.0;

    /// The whole comfort argument is that the middle of the field is left
    /// alone. If the ramp ever started at the view axis this would be the
    /// full-screen flash the design rejects.
    ///
    /// And - the bug that shipped in the first cut and was caught by looking at
    /// a flat screenshot - the ramp has to land *inside the picture*. Sharing
    /// one absolute angle across both presentations put flat's whole ramp past
    /// the edge of a 45-degree screen, so a hit rendered as nothing at all.
    #[test]
    fn the_tint_lands_inside_the_picture_at_any_field_of_view() {
        for half_field in [FLAT_HALF_FIELD, VR_HALF_FIELD, 75.0] {
            let inner = radius_at_half_angle(half_field * CLEAR_FIELD_FRACTION);
            let outer = radius_at_half_angle(half_field * FULL_FIELD_FRACTION);
            assert!(
                inner > 0.0,
                "{half_field}: the tint must not start on the view axis"
            );
            assert!(outer > inner, "{half_field}: the ramp must have width");
            assert!(
                half_angle_at_radius(inner) < cgmath::Deg(half_field * 0.75),
                "{half_field}: the ramp starts too near the edge of the picture to be seen"
            );
            assert!(
                half_angle_at_radius(outer) <= cgmath::Deg(half_field + 1.0),
                "{half_field}: the tint never reaches full strength inside the picture"
            );
        }
    }

    /// Flat and VR must tint the *same fraction* of the picture - that is what
    /// "renders identically" means for an effect defined in the field of view
    /// rather than in pixels.
    #[test]
    fn every_field_of_view_tints_the_same_fraction_of_the_picture() {
        let fractions: Vec<f32> = [FLAT_HALF_FIELD, VR_HALF_FIELD]
            .into_iter()
            .map(|half_field| {
                half_angle_at_radius(radius_at_half_angle(half_field * CLEAR_FIELD_FRACTION)).0
                    / half_field
            })
            .collect();
        assert!((fractions[0] - fractions[1]).abs() < 1e-3, "{fractions:?}");
    }

    /// The flat runtimes' own projection must come back out as the flat field.
    #[test]
    fn the_field_is_read_off_the_projection_the_host_renders_with() {
        let flat = cgmath::perspective(cgmath::Deg(45.0), 800.0 / 600.0, 0.1, 1000.0);
        assert!((half_field_deg_from_projection(flat) - FLAT_HALF_FIELD).abs() < 1.0);

        // A wide headset frustum reports a wide field...
        let wide = cgmath::perspective(cgmath::Deg(90.0), 1.0, 0.1, 1000.0);
        assert!(half_field_deg_from_projection(wide) > VR_HALF_FIELD * 0.7);

        // ...and a degenerate matrix falls back instead of producing NaN.
        assert_eq!(
            half_field_deg_from_projection(Matrix4::from_scale(0.0)),
            DEFAULT_HALF_FIELD_DEG
        );
    }

    /// #1020's bug in miniature: a layer that relies on the host agreeing
    /// about winding covers only part of the view on the Quest.
    #[test]
    fn the_layer_does_not_depend_on_backface_culling() {
        let object = hit_layer(VR_HALF_FIELD, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        assert_eq!(object.backface_culling(), None);
    }

    #[test]
    fn the_layer_draws_translucent_and_over_the_world() {
        let object = hit_layer(VR_HALF_FIELD, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
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
            let object = hit_layer(VR_HALF_FIELD, eye, forward, 0.5);
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
