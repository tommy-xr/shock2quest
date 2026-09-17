//! The flat crosshair, drawn as four arms that separate to show how wide the
//! shot can land and shift to show where recoil has pushed it.
//!
//! There are three error sources, of two different kinds, and the reticle must
//! not conflate them:
//!
//! - a **random** square of heading/pitch error - the authored weapon error
//!   (`CalcRandAngle`, zero in stock data) plus the per-pellet spread of a
//!   multi-pellet shell - which the arms show as an outward *bloom*;
//! - a **deterministic** directional bias - the recoil the shot rides
//!   (`weapon_recoil`) - which the whole reticle shows as a *shift*.
//!
//! Drawing them as one number would be a lying reticle: a bloom says "somewhere
//! in here at random", a shift says "there". The centre dot never moves, so the
//! camera axis stays readable underneath both.
//!
//! Every number here is read from the same state the shot itself uses, never
//! recomputed in parallel - see `reticle_matches_the_shot` in the e2e suite.

use cgmath::{Vector2, vec2};

use crate::ui::{Rect, UiCanvas};

/// The art is four capsule arms around a centre dot. These are its own texels
/// (`iface/CROSSHAI`, authored 32x32, the arms measured at x/y 12..20): a
/// partition of the sprite into the four arms and the 8x8 square where they
/// cross, so the pieces reconstruct it exactly when nothing is blooming.
///
/// Two cuts that look right and are not: **quadrants** tear each arm along its
/// own axis (the top arm straddles the vertical centreline), and **full-length
/// bands** overlap each other in that centre square, drawing it twice.
const ART: Vector2<f32> = vec2(32.0, 32.0);
const ARM_TOP: Rect = Rect::new(12.0, 0.0, 8.0, 12.0);
const ARM_BOTTOM: Rect = Rect::new(12.0, 20.0, 8.0, 12.0);
const ARM_LEFT: Rect = Rect::new(0.0, 12.0, 12.0, 8.0);
const ARM_RIGHT: Rect = Rect::new(20.0, 12.0, 12.0, 8.0);
/// Where the four arms cross: the centre dot plus the arms' inner tips. Held
/// fixed on the camera axis, so a blooming reticle keeps a small still core to
/// read the moving arms against.
const CENTRE_DOT: Rect = Rect::new(12.0, 12.0, 8.0, 8.0);

/// Dark stores angles as u16 turns.
const RADIANS_PER_UNIT: f32 = std::f32::consts::TAU / 65536.0;

/// What the reticle is advertising, in angles - so it is comparable with the
/// fire ray directly, independent of resolution or field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ReticleState {
    /// Half-width of the random heading/pitch error square, in radians. The
    /// distribution really is a square (heading and pitch are drawn
    /// independently), so the arms mark its edge along their own axis rather
    /// than approximating a cone.
    pub spread: f32,
    /// Where recoil has pushed the shot relative to the camera axis, in
    /// radians: +x right, +y up.
    pub bias: Vector2<f32>,
}

impl Default for ReticleState {
    fn default() -> Self {
        Self {
            spread: 0.0,
            bias: vec2(0.0, 0.0),
        }
    }
}

impl ReticleState {
    /// Sum the authored weapon error and the per-pellet spread. `expand`
    /// deviates each pellet on top of the already-deviated shell, so a pellet's
    /// worst error along one axis is the sum of the two half-widths.
    pub(crate) fn from_units(weapon_error: u16, pellet_spread: u16, bias: Vector2<f32>) -> Self {
        Self {
            spread: (u32::from(weapon_error) + u32::from(pellet_spread)) as f32 * RADIANS_PER_UNIT,
            bias: if bias.x.is_finite() && bias.y.is_finite() {
                bias
            } else {
                vec2(0.0, 0.0)
            },
        }
    }
}

/// Read the live reticle from the wielded weapon: the two random error
/// sources the launch path will apply, and the recoil deflection the flat
/// controller recorded for the ray it will fire along.
///
/// Both spreads come from the same places `create_projectile` reads them -
/// `weapon_inaccuracy` for the authored per-shot weapon error and the
/// projectile archetype's own `spread` for a multi-pellet shell - so the
/// reticle cannot drift from the shot.
pub(crate) fn from_world(
    world: &shipyard::World,
    weapon: Option<shipyard::EntityId>,
    bias: Vector2<f32>,
) -> ReticleState {
    use shipyard::{Get, UniqueView, View};
    let Some(weapon) = weapon else {
        return ReticleState::default();
    };
    let pellet_spread = crate::scripts::script_util::ordered_projectile_links(world, weapon)
        .get(
            world
                .borrow::<View<crate::runtime_props::RuntimePropSelectedAmmo>>()
                .ok()
                .and_then(|v| v.get(weapon).ok().map(|s| s.0))
                .unwrap_or(0)
                % crate::scripts::script_util::ordered_projectile_links(world, weapon)
                    .len()
                    .max(1),
        )
        .and_then(|(template, _)| {
            world
                .borrow::<UniqueView<crate::mission::projectile_spray::GlobalProjectileSprays>>()
                .ok()
                .and_then(|sprays| sprays.0.get(template).map(|p| p.spread))
        })
        .unwrap_or(0);
    ReticleState::from_units(
        crate::scripts::weapon_script::weapon_inaccuracy(world, weapon),
        pellet_spread,
        bias,
    )
}

/// Project an angle off the view axis onto the HUD canvas, in canvas pixels.
/// This is the inverse of the perspective divide the world is drawn with, so a
/// shot leaving at `angle` lands under the arm drawn at the returned offset.
pub(crate) fn angle_to_canvas_px(angle: f32, fov_y_degrees: f32, canvas_height: f32) -> f32 {
    let half_fov = (fov_y_degrees.clamp(1.0, 179.0) / 2.0).to_radians();
    if !angle.is_finite() {
        return 0.0;
    }
    // Clamp short of a right angle: tan diverges, and an arm a thousand
    // screens away is not a readout.
    let angle = angle.clamp(-half_fov, half_fov);
    (canvas_height / 2.0) * angle.tan() / half_fov.tan()
}

/// Draw the crosshair into `canvas`, centred on `centre`, at `size` canvas
/// pixels square. With a zero state this emits the sprite exactly as one
/// undivided image would.
pub(crate) fn emit(
    canvas: &mut UiCanvas,
    centre: Vector2<f32>,
    size: f32,
    texture: &str,
    state: ReticleState,
    fov_y_degrees: f32,
    canvas_height: f32,
) {
    let scale = size / ART.x;
    let bloom = angle_to_canvas_px(state.spread, fov_y_degrees, canvas_height).max(0.0);
    let shift = vec2(
        angle_to_canvas_px(state.bias.x, fov_y_degrees, canvas_height),
        // Canvas y grows downward; a shot pushed UP draws higher on screen.
        -angle_to_canvas_px(state.bias.y, fov_y_degrees, canvas_height),
    );
    // The sprite's own top-left if it were drawn undivided and unbloomed.
    let origin = centre - vec2(size, size) / 2.0 + shift;
    let mut piece = |source: Rect, push: Vector2<f32>| {
        canvas.cropped_image(
            Rect::new(
                origin.x + source.x * scale + push.x,
                origin.y + source.y * scale + push.y,
                source.w * scale,
                source.h * scale,
            ),
            texture,
            source,
            ART,
        );
    };
    piece(ARM_TOP, vec2(0.0, -bloom));
    piece(ARM_BOTTOM, vec2(0.0, bloom));
    piece(ARM_LEFT, vec2(-bloom, 0.0));
    piece(ARM_RIGHT, vec2(bloom, 0.0));
    // The dot marks true camera aim, so it takes neither the bloom nor the
    // shift: it is the fixed reference the other four are read against.
    canvas.cropped_image(
        Rect::new(
            centre.x - size / 2.0 + CENTRE_DOT.x * scale,
            centre.y - size / 2.0 + CENTRE_DOT.y * scale,
            CENTRE_DOT.w * scale,
            CENTRE_DOT.h * scale,
        ),
        texture,
        CENTRE_DOT,
        ART,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    fn rects(canvas: &UiCanvas) -> Vec<(f32, f32, f32, f32)> {
        canvas
            .elements()
            .iter()
            .filter_map(|e| match e {
                UiElement::Image { position, size, .. } => {
                    Some((position.x, position.y, size.x, size.y))
                }
                _ => None,
            })
            .collect()
    }

    fn draw(state: ReticleState) -> Vec<(f32, f32, f32, f32)> {
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        emit(
            &mut canvas,
            vec2(320.0, 240.0),
            32.0,
            "CROSSHAI.PCX",
            state,
            45.0,
            480.0,
        );
        rects(&canvas)
    }

    /// The four bands plus the dot must tile the sprite exactly when nothing is
    /// blooming, so turning the feature off is pixel-identical to the single
    /// undivided image the HUD used to draw.
    #[test]
    fn a_resting_reticle_reassembles_the_original_sprite() {
        let pieces = draw(ReticleState::default());
        assert_eq!(pieces.len(), 5);
        let (x0, y0) = (320.0 - 16.0, 240.0 - 16.0);
        for (source, rect) in [ARM_TOP, ARM_BOTTOM, ARM_LEFT, ARM_RIGHT, CENTRE_DOT]
            .into_iter()
            .zip(&pieces)
        {
            assert_eq!(*rect, (x0 + source.x, y0 + source.y, source.w, source.h));
        }
        // No band overlaps another: at rest each texel is drawn exactly once.
        for (i, a) in pieces.iter().enumerate() {
            for b in &pieces[i + 1..] {
                let overlap = (a.0 + a.2).min(b.0 + b.2) > a.0.max(b.0)
                    && (a.1 + a.3).min(b.1 + b.3) > a.1.max(b.1);
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
    }

    /// Bloom pushes each arm out along its OWN axis, never sideways, and never
    /// moves the centre dot.
    #[test]
    fn spread_separates_the_arms_symmetrically_around_a_fixed_centre() {
        let rest = draw(ReticleState::default());
        let spread = 4.0_f32.to_radians();
        let bloomed = draw(ReticleState {
            spread,
            bias: vec2(0.0, 0.0),
        });
        let push = angle_to_canvas_px(spread, 45.0, 480.0);
        assert!(push > 1.0, "a 4 degree cone should be visible: {push}");
        let expected = [
            (0.0, -push),
            (0.0, push),
            (-push, 0.0),
            (push, 0.0),
            (0.0, 0.0), // the dot
        ];
        for ((r, b), (dx, dy)) in rest.iter().zip(&bloomed).zip(expected) {
            assert!((b.0 - r.0 - dx).abs() < 1e-3, "x: {r:?} -> {b:?}");
            assert!((b.1 - r.1 - dy).abs() < 1e-3, "y: {r:?} -> {b:?}");
            assert_eq!((b.2, b.3), (r.2, r.3), "bloom must not resize an arm");
        }
    }

    /// Recoil shifts the four arms together - the shot really did go one way -
    /// while the dot stays on the camera axis as the reference to read against.
    #[test]
    fn recoil_bias_shifts_the_arms_but_never_the_centre_dot() {
        let rest = draw(ReticleState::default());
        let up = 2.0_f32.to_radians();
        let shifted = draw(ReticleState {
            spread: 0.0,
            bias: vec2(0.0, up),
        });
        let push = angle_to_canvas_px(up, 45.0, 480.0);
        assert!(push > 1.0);
        for (i, (r, s)) in rest.iter().zip(&shifted).enumerate() {
            let expected = if i == 4 { 0.0 } else { -push };
            assert!(
                (s.1 - r.1 - expected).abs() < 1e-3,
                "piece {i}: {r:?} -> {s:?}"
            );
            assert!((s.0 - r.0).abs() < 1e-3, "an upward bias must not move x");
        }
    }

    /// The projection is the inverse of the world's: an arm at the drawn offset
    /// sits exactly where a shot leaving at that angle lands.
    #[test]
    fn the_projection_inverts_the_perspective_divide() {
        for fov in [30.0_f32, 45.0, 90.0] {
            let half = (fov / 2.0).to_radians();
            // The edge of the frustum is the edge of the canvas.
            assert!((angle_to_canvas_px(half, fov, 480.0) - 240.0).abs() < 0.01);
            assert_eq!(angle_to_canvas_px(0.0, fov, 480.0), 0.0);
            // Monotonic, odd, and never past the frustum edge.
            let (a, b) = (
                angle_to_canvas_px(half / 3.0, fov, 480.0),
                angle_to_canvas_px(half / 2.0, fov, 480.0),
            );
            assert!(0.0 < a && a < b && b < 240.0);
            assert!((angle_to_canvas_px(-half / 3.0, fov, 480.0) + a).abs() < 1e-4);
            assert!(angle_to_canvas_px(3.0, fov, 480.0) <= 240.001);
        }
        assert_eq!(angle_to_canvas_px(f32::NAN, 45.0, 480.0), 0.0);
    }

    /// Weapon error and pellet spread add: `expand` deviates every pellet on
    /// top of the shell's already-deviated launch.
    #[test]
    fn the_advertised_spread_sums_both_random_sources() {
        let unit = RADIANS_PER_UNIT;
        let both = ReticleState::from_units(128, 64, vec2(0.0, 0.0));
        assert!((both.spread - 192.0 * unit).abs() < 1e-9);
        assert_eq!(
            ReticleState::from_units(0, 0, vec2(0.0, 0.0)),
            ReticleState::default()
        );
        // Stock data zeroes the weapon error; a shotgun still blooms.
        assert!(ReticleState::from_units(0, 512, vec2(0.0, 0.0)).spread > 0.0);
        // u16 + u16 must not wrap.
        assert!(ReticleState::from_units(u16::MAX, u16::MAX, vec2(0.0, 0.0)).spread > 0.0);
        // A malformed bias must never reach the canvas.
        assert_eq!(
            ReticleState::from_units(0, 0, vec2(f32::NAN, 1.0)).bias,
            vec2(0.0, 0.0)
        );
    }
}
