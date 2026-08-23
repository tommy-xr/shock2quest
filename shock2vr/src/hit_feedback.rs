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
use engine::scene::{RenderLayer, SceneObject};

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

/// How far in front of the eyes the layer hangs, in metres.
///
/// Far rather than near, and for one reason: **stereo**. This is one quad
/// shared by both eyes, hung from the runtime's head pose, so an eye 32 mm off
/// that pose sees it shifted by `atan(0.032 / distance)`. At half a metre that
/// is 3.7 degrees of disagreement between the eyes about where the rim is; out
/// here it is under a degree, the same margin the pause menu's dim relies on.
/// Depth is not a consideration - the layer clears depth, so nothing in the
/// world can occlude it at any distance.
const LAYER_DISTANCE: f32 = 2.5;

/// How much bigger than the frustum the quad is cut. The quad is sized to the
/// picture exactly (see [`hit_layer`]), so a little slack absorbs a head that
/// turned between the pose the layer was hung from and the pose the frame is
/// rendered at, and any small disagreement between a host's reported and
/// actual frustum. It costs nothing but a few pixels of fully-tinted margin.
const COVERAGE_MARGIN: f32 = 1.25;

/// Where the tint starts and where it reaches full strength, as fractions of
/// the way from the view axis to the edge of the picture. Everything inside
/// [`CLEAR_FIELD_FRACTION`] is untouched - that is the part of the field the
/// player aims and reads with - and the tint is at full strength by the edge.
///
/// Being *fractions of the picture* is what makes this render identically in
/// flat and VR (AGENTS.md section 3) without a per-presentation branch: the
/// picture's own extents come from the host's projection matrix, so the same
/// two numbers describe the same visible effect on a 45-degree monitor and on
/// a headset's much wider asymmetric per-eye frustum.
pub const CLEAR_FIELD_FRACTION: f32 = 0.5;
pub const FULL_FIELD_FRACTION: f32 = 1.0;

/// How far the picture reaches from the view axis, as the tangents of the
/// half-angles on each axis: `(horizontal, vertical)`.
///
/// The fallback is what both flat runtimes' `perspective(Deg(45.0))` on a 4:3
/// target gives, for the frames before any projection has been seen.
pub const DEFAULT_VIEW_EXTENTS: (f32, f32) = (0.552_28, 0.414_21);

/// Read [`DEFAULT_VIEW_EXTENTS`]'s quantity off the projection the host is
/// actually rendering with.
///
/// Hardcoding one angle is what the first cut did, and it made the tint
/// invisible on flat: the ramp was authored for a headset's ~55 degrees and
/// the whole of it sat past the edge of a 45-degree screen. Hardcoding *two*
/// (one per presentation) would then have been a lie in the debug runtime,
/// whose `--vr` mode renders with the flat projection. Asking the projection
/// is right everywhere.
///
/// For any perspective or off-axis frustum, `m[0][0]` is `2n / (r - l)` and
/// `m[1][1]` is `2n / (t - b)`, so their reciprocals are the tangents of half
/// the horizontal and vertical extents. Reading **both** matters: a 4:3 screen
/// reaches 29 degrees sideways but only 22.5 up, and a single radius would
/// tint a different fraction of the picture on each axis.
pub fn view_extents_from_projection(projection: Matrix4<f32>) -> (f32, f32) {
    let extent = |scale: f32, fallback: f32| {
        if scale.is_finite() && scale > 0.0 {
            (1.0 / scale).clamp(0.05, 10.0)
        } else {
            fallback
        }
    };
    (
        extent(projection.x.x, DEFAULT_VIEW_EXTENTS.0),
        extent(projection.y.y, DEFAULT_VIEW_EXTENTS.1),
    )
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
    /// A second hit while one is still showing restarts the decay, and can
    /// only ever make the tint *stronger or longer*, never weaker or shorter:
    /// taking a graze mid-fade from a shotgun blast must not look like the
    /// blast stopped hurting, and must not cut its fade short either. So both
    /// the peak and the lifetime are the larger of the incoming hit and what
    /// is already on screen.
    pub fn trigger(&mut self, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        self.peak = peak_opacity(damage).max(self.intensity());
        self.duration = duration_secs(damage).max(self.remaining);
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
    /// pause panel. `view_extents` comes from the host's own projection - see
    /// [`view_extents_from_projection`].
    pub fn render(
        &self,
        view_extents: (f32, f32),
        eye_position: Vector3<f32>,
        eye_forward: Vector3<f32>,
    ) -> Option<SceneObject> {
        let intensity = self.intensity();
        if intensity <= 0.0 {
            return None;
        }
        Some(hit_layer(
            view_extents,
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
/// differs, and the quad is cut to whatever picture the host is rendering (see
/// [`view_extents_from_projection`]). Nothing else does.
///
/// The quad is cut to the frustum on **each axis separately** and the ramp is
/// then a plain circle in the quad's own UVs, which is what makes the tint
/// cover the same fraction of the picture sideways and vertically on a 4:3
/// screen (29 degrees across, 22.5 up) as it does on a near-square headset eye.
fn hit_layer(
    view_extents: (f32, f32),
    eye_position: Vector3<f32>,
    eye_forward: Vector3<f32>,
    intensity: f32,
) -> SceneObject {
    vignette_layer(
        view_extents,
        eye_position,
        eye_forward,
        TINT_COLOR,
        intensity,
        CLEAR_FIELD_FRACTION,
        FULL_FIELD_FRACTION,
        crate::util::render_source::HIT_FEEDBACK,
    )
}

/// One view-locked, double-sided rim-vignette quad - the shared geometry
/// behind both [`hit_layer`] (the damage tint) and the cyber interface's own
/// entry/exit vignette ([`crate::ui::entry_ramp`]). The two are drawn as
/// separate layers with their own color/intensity rather than merged into one
/// number, so a hit still reads while the interface is open (they blend
/// naturally, being translucent).
///
/// See [`hit_layer`]'s callers for what each parameter means; `clear_field`
/// and `full_field` are fractions of the picture (0..1) the same way
/// [`CLEAR_FIELD_FRACTION`]/[`FULL_FIELD_FRACTION`] are.
#[allow(clippy::too_many_arguments)]
pub fn vignette_layer(
    view_extents: (f32, f32),
    eye_position: Vector3<f32>,
    eye_forward: Vector3<f32>,
    color: Vector3<f32>,
    intensity: f32,
    clear_field: f32,
    full_field: f32,
    source: &str,
) -> SceneObject {
    let (horizontal, vertical) = view_extents;
    let width = 2.0 * LAYER_DISTANCE * horizontal * COVERAGE_MARGIN;
    let height = 2.0 * LAYER_DISTANCE * vertical * COVERAGE_MARGIN;
    let mut object = SceneObject::new(
        engine::scene::vignette_material::create(
            color,
            intensity,
            // The quad is `COVERAGE_MARGIN` wider than the picture, so the
            // edge of the picture sits at `1 / COVERAGE_MARGIN` in the shader's
            // radius units and the fractions scale down to match.
            clear_field / COVERAGE_MARGIN,
            full_field / COVERAGE_MARGIN,
        ),
        Box::new(engine::scene::quad::create()),
    );
    object.set_transform(
        Matrix4::from_translation(eye_position + eye_forward * LAYER_DISTANCE)
            // The quad's +Z faces the viewer once it is turned to look back
            // along the gaze.
            * Matrix4::from(crate::util::get_rotation_from_forward_vector(-eye_forward))
            * Matrix4::from_nonuniform_scale(width, height, 1.0),
    );
    // Translucent: writing depth here would let the layer occlude anything
    // drawn after it in the same overlay group.
    object.set_depth_write(false);
    // The layer covers the world but stays behind scene-owned UI. The renderer
    // orders this key explicitly, so host concatenation cannot move the tint
    // over the HUD on Quest or under the world on flat/debug.
    object.set_render_layer(RenderLayer::SceneOverlay);
    object.set_debug_tag(Some(crate::util::render_source_tag(source)));
    object
}

/// The eye pose to hang the layer from, in pawn space.
///
/// An untracked head has no usable pose (see [`crate::util::tracked_gaze`]);
/// the pawn's own default eye, looking straight ahead, stands in for it.
pub fn eye_pose(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    crate::util::tracked_gaze(head_position, head_rotation).unwrap_or((
        vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
        vec3(0.0, 0.0, -1.0),
    ))
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
                .render(VR_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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
                .render(VR_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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
                .render(VR_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0),)
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

    /// ...and it must not cut the fade short either: a 1-point graze arriving
    /// a quarter of the way through a shotgun blast's fade would otherwise
    /// replace 0.64 s of remaining tint with the graze's own 0.39 s.
    #[test]
    fn a_graze_mid_fade_never_shortens_the_fade() {
        let mut feedback = HitFeedback::new();
        feedback.trigger(12.0);
        feedback.update(duration_secs(12.0) * 0.25);
        let remaining_before = feedback.remaining;

        feedback.trigger(1.0);
        assert!(
            feedback.remaining >= remaining_before,
            "a graze cut the fade from {remaining_before} to {}",
            feedback.remaining
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

    /// What both flat runtimes' `perspective(Deg(45.0))` on a 4:3 target
    /// gives, and roughly what one Quest eye reaches. Used to stand in for a
    /// monitor and for a headset.
    const FLAT_EXTENTS: (f32, f32) = DEFAULT_VIEW_EXTENTS;
    const VR_EXTENTS: (f32, f32) = (1.428, 1.428);

    /// Where the picture's own edge lands in the shader's radius units.
    const PICTURE_EDGE_RADIUS: f32 = 1.0 / COVERAGE_MARGIN;

    /// The whole comfort argument is that the middle of the field is left
    /// alone. If the ramp ever started at the view axis this would be the
    /// full-screen flash the design rejects.
    ///
    /// And - the bug that shipped in the first cut and was caught by looking at
    /// a flat screenshot - the ramp has to land *inside the picture*. Authoring
    /// it as an absolute angle put flat's whole ramp past the edge of a
    /// 45-degree screen, so a hit rendered as nothing at all.
    #[test]
    fn the_tint_lands_inside_the_picture() {
        let inner = CLEAR_FIELD_FRACTION / COVERAGE_MARGIN;
        let outer = FULL_FIELD_FRACTION / COVERAGE_MARGIN;
        assert!(inner > 0.0, "the tint must not start on the view axis");
        assert!(outer > inner, "the ramp must have width");
        assert!(
            inner < PICTURE_EDGE_RADIUS * 0.75,
            "the ramp starts too near the edge of the picture to be seen"
        );
        assert!(
            outer <= PICTURE_EDGE_RADIUS,
            "the tint never reaches full strength inside the picture"
        );
    }

    /// The quad has to cover the picture it is cut for - on both axes, and
    /// with room for the head to have turned since the pose it was hung from.
    /// A quad that fell short would leave an untinted band at the edge, which
    /// is exactly where the effect lives.
    #[test]
    fn the_quad_covers_the_whole_picture_on_both_axes() {
        for extents in [FLAT_EXTENTS, VR_EXTENTS] {
            let object = hit_layer(extents, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
            let transform = object.get_transform();
            // The quad spans -0.5..0.5 before its scale, so a corner maps to
            // the half-extents.
            let corner = transform * cgmath::vec4(0.5, 0.5, 0.0, 1.0);
            let picture_half_width = LAYER_DISTANCE * extents.0;
            let picture_half_height = LAYER_DISTANCE * extents.1;
            assert!(
                corner.x.abs() > picture_half_width,
                "{extents:?}: the quad is narrower than the picture"
            );
            assert!(
                corner.y.abs() > picture_half_height,
                "{extents:?}: the quad is shorter than the picture"
            );
        }
    }

    /// A 4:3 monitor reaches further sideways than up, so a quad cut to one
    /// axis would tint a different fraction of the picture on the other. The
    /// quad is cut per axis precisely so the circular UV ramp lands at the
    /// same fraction of the way to the edge everywhere.
    #[test]
    fn the_quad_is_cut_to_the_pictures_own_aspect() {
        let object = hit_layer(FLAT_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        let corner = object.get_transform() * cgmath::vec4(0.5, 0.5, 0.0, 1.0);
        let quad_aspect = corner.x.abs() / corner.y.abs();
        let picture_aspect = FLAT_EXTENTS.0 / FLAT_EXTENTS.1;
        assert!(
            (quad_aspect - picture_aspect).abs() < 1e-3,
            "quad aspect {quad_aspect} does not match the picture's {picture_aspect}"
        );
    }

    /// The flat runtimes' own projection must come back out as the flat
    /// picture, on both axes.
    #[test]
    fn the_picture_is_read_off_the_projection_the_host_renders_with() {
        let flat = cgmath::perspective(cgmath::Deg(45.0), 800.0 / 600.0, 0.1, 1000.0);
        let (horizontal, vertical) = view_extents_from_projection(flat);
        assert!((horizontal - FLAT_EXTENTS.0).abs() < 0.01);
        assert!((vertical - FLAT_EXTENTS.1).abs() < 0.01);
        assert!(
            horizontal > vertical,
            "a 4:3 picture is wider than it is tall"
        );

        // A wide headset frustum reports a wide picture...
        let wide = cgmath::perspective(cgmath::Deg(90.0), 1.0, 0.1, 1000.0);
        assert!(view_extents_from_projection(wide).0 > horizontal * 1.5);

        // ...and a degenerate matrix falls back instead of producing NaN.
        assert_eq!(
            view_extents_from_projection(Matrix4::from_scale(0.0)),
            DEFAULT_VIEW_EXTENTS
        );
    }

    /// #1020's bug in miniature: a layer that relies on the host agreeing
    /// about winding covers only part of the view on the Quest.
    #[test]
    fn the_layer_does_not_depend_on_backface_culling() {
        let object = hit_layer(VR_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        assert_eq!(object.backface_culling(), None);
    }

    #[test]
    fn the_layer_draws_translucent_and_over_the_world() {
        let object = hit_layer(VR_EXTENTS, Vector3::zero(), vec3(0.0, 0.0, -1.0), 0.5);
        let transparency = object
            .effective_transparency()
            .expect("the tint must draw translucent, not opaque");
        assert!(transparency > 0.0 && transparency < 1.0);
        assert_eq!(object.render_layer(), RenderLayer::SceneOverlay);
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
            let object = hit_layer(VR_EXTENTS, eye, forward, 0.5);
            let centre = object.get_transform() * cgmath::vec4(0.0, 0.0, 0.0, 1.0);
            let offset = vec3(centre.x, centre.y, centre.z) - eye;
            assert!(
                (offset.normalize() - forward).magnitude() < 1e-3,
                "layer centre {offset:?} is not along the gaze {forward:?}"
            );
            assert!((offset.magnitude() - LAYER_DISTANCE).abs() < 1e-3);
        }
    }

    /// `Game::render` clamps the layer's eye to the same cap the cameras
    /// clamp theirs to, because the input context reports the *standing* eye
    /// even while the frame is drawn from a crouched one. Pin that the clamp
    /// resolves to the flat runtime's crouched eye line, and that it does
    /// nothing at all while standing.
    #[test]
    fn the_layer_eye_is_clamped_to_the_eye_the_camera_renders_from() {
        let standing = crate::physics::player_eye_cap_above_center(false);
        let crouched = crate::physics::player_eye_cap_above_center(true);
        let reported = crate::input_context::DEFAULT_HEAD_HEIGHT;

        assert!(
            reported.min(standing) == reported,
            "the clamp must be a no-op standing: reported {reported}, cap {standing}"
        );
        assert!(
            reported.min(crouched) < reported,
            "crouched, the reported eye must be pulled down to the camera's"
        );
        assert!(
            (crouched - crate::PLAYER_CROUCH_EYE_HEIGHT / dark::SCALE_FACTOR).abs() < 1e-6,
            "the crouched cap is the flat camera's crouched eye"
        );
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

    /// [`vignette_layer`] is the shared geometry `hit_layer` and the cyber
    /// interface's own rim tint both build on - a caller with different
    /// color/fractions/source gets a layer tagged and colored as its own,
    /// not silently relabeled as hit feedback.
    #[test]
    fn vignette_layer_carries_the_callers_own_color_and_source() {
        let color = vec3(0.05, 0.35, 0.55);
        let object = vignette_layer(
            VR_EXTENTS,
            Vector3::zero(),
            vec3(0.0, 0.0, -1.0),
            color,
            0.4,
            0.3,
            0.9,
            "use_mode_vignette",
        );
        assert_eq!(
            object.debug_tag().and_then(|tag| tag.source.clone()),
            Some("use_mode_vignette".to_owned())
        );
        let transparency = object
            .effective_transparency()
            .expect("must draw translucent");
        assert!((transparency - 0.6).abs() < 1e-5);
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
