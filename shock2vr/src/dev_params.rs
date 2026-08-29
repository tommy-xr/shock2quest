//! Live-tunable developer parameters.
//!
//! A small registry of runtime tuning knobs that used to be module-private
//! `const`s (and so cost a rebuild - and on the Quest, a redeploy - per
//! nudge). Each parameter is declared once in the [`dev_params!`] invocation
//! below; consumers read it through [`get`] every frame, so a [`set`] is
//! visible on the next frame with no plumbing.
//!
//! Storage is a static table plus one `AtomicU32` of `f32` bits per parameter:
//! lock-free, safe from any thread, and needing no `&mut Game`. That is what
//! lets the debug runtime's `GET`/`POST /v1/dev-params` handlers read and
//! write the registry directly (no `RuntimeCommand` round-trip), and what will
//! let the Developer menu screen do the same from the game thread.
//!
//! Reads are individually atomic but deliberately unsynchronized across a
//! frame: a [`set`] landing mid-frame can be seen by later reads in the same
//! frame (e.g. a panel hit-test and its render disagreeing by one frame).
//! That is acceptable for tuning knobs - the next frame is consistent - and
//! in the stepped debug runtime it never happens at all, because HTTP sets
//! land between frames.
//!
//! Ground rule: **if the consumer does not read the parameter every frame, it
//! does not belong in this table yet.** Values latched at construction time
//! (feature gates, collider dimensions, anything baked into a scene object)
//! would need a poke/rebuild path this registry deliberately does not have.

use std::sync::atomic::{AtomicU32, Ordering};

/// How a parameter's value is interpreted and constrained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevParamKind {
    /// A float clamped to `min..=max`; [`set`] snaps to the nearest multiple
    /// of `step` from `min` (the grid the future menu's `<`/`>` arrows walk).
    Float { min: f32, max: f32, step: f32 },
    /// An on/off switch, stored in the same f32 bits as 0.0/1.0. Anything
    /// non-zero [`set`] receives becomes 1.0, so a caller cannot store a
    /// third state.
    Bool,
    // `Enum(&'static [&'static str])` goes here when a param needs it -
    // stored as the variant index in the same bits.
}

/// One registered parameter: a stable string key (HTTP + future persistence),
/// a human-readable label (the future menu row), its kind, and its default.
#[derive(Debug)]
pub struct DevParam {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: DevParamKind,
    pub default: f32,
}

/// Index into [`PARAMS`]; obtained from the generated consts or [`find`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevParamId(usize);

/// Declares the parameter table. One line per parameter generates its
/// [`PARAMS`] entry *and* its `pub const` id, so the two cannot drift.
/// One declaration's [`DevParam`] entry. Split out of [`dev_params!`] so the
/// table can mix kinds: the outer macro matches each line as
/// `kind(args...)` and defers the shape of `args` to these arms.
macro_rules! dev_param_entry {
    (float($key:literal, $label:literal, $default:expr, $min:expr, $max:expr, $step:expr)) => {
        DevParam {
            key: $key,
            label: $label,
            kind: DevParamKind::Float {
                min: $min,
                max: $max,
                step: $step,
            },
            default: dev_param_default!(float($key, $label, $default, $min, $max, $step)),
        }
    };
    (bool($key:literal, $label:literal, $default:expr)) => {
        DevParam {
            key: $key,
            label: $label,
            kind: DevParamKind::Bool,
            default: dev_param_default!(bool($key, $label, $default)),
        }
    };
}

/// One declaration's default, as the `f32` the value table stores. A `bool`
/// cannot be `as f32`, and this is the one place that conversion belongs.
macro_rules! dev_param_default {
    (float($key:literal, $label:literal, $default:expr, $min:expr, $max:expr, $step:expr)) => {
        ($default as f32)
    };
    (bool($key:literal, $label:literal, $default:expr)) => {
        if $default { 1.0f32 } else { 0.0f32 }
    };
}

macro_rules! dev_params {
    ($($(#[$doc:meta])* $id:ident = $kind:ident ( $($args:tt)* )),+ $(,)?) => {
        /// Every registered parameter, in declaration order (index == id).
        pub static PARAMS: &[DevParam] = &[
            $(dev_param_entry!($kind($($args)*))),+
        ];

        /// Current values, as `f32` bits, seeded with the defaults. A sized
        /// array, not a `&[_]` slice: a borrow of interior-mutable data
        /// cannot be promoted to `'static` (E0492).
        static VALUES: [AtomicU32; [$(dev_param_default!($kind($($args)*))),+].len()] =
            [$(AtomicU32::new(dev_param_default!($kind($($args)*)).to_bits())),+];

        #[allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
        enum __DevParamIndex { $($id),+ }

        $($(#[$doc])* pub const $id: DevParamId = DevParamId(__DevParamIndex::$id as usize);)+
    };
}

dev_params! {
    /// How far ahead of the head the VR frontend/pause panel hangs, in world
    /// units. Default matches the old `ui::FRONTEND_PANEL_DISTANCE` const.
    FRONTEND_PANEL_DISTANCE = float("panel_distance", "Panel distance", 2.0, 0.5, 6.0, 0.1),
    /// How dark the world goes behind the pause panel in VR: 0 leaves it
    /// untouched, 1 blacks it out. Default matches the old
    /// `pause_menu::WORLD_DIM_STRENGTH` const.
    WORLD_DIM_STRENGTH = float("dim_strength", "Pause dim", 0.72, 0.0, 1.0, 0.02),
    /// Vertical offset applied to the whole tracked stage on the Quest, in
    /// **meters** (the stage's own unit): head, hands and the per-eye view
    /// move together, because `oculus_runtime` adds it inside its single
    /// stage-to-pawn mapping. The per-eye cap that holds a *tracked* head
    /// inside the collider crown is raised by the same offset, so the knob
    /// moves the view by exactly what it moves the hands by rather than
    /// saturating the view a couple of centimeters in (standing headroom above
    /// the collider center is only ~0.85 m, which an adult's tracked eye
    /// already nearly fills). At the default 0 the cap is unchanged.
    /// Deliberately NOT read by
    /// `player_eye_height_for` (the flat/debug camera): on the Quest the eye
    /// is the tracked pose, which never goes through that accessor - wiring
    /// the accessor instead would leave the knob inert exactly where live
    /// tuning matters while the debug runtime falsely "verified" it (the PR1
    /// deferral). Consequently the debug runtime cannot exercise this knob;
    /// it needs a worn check on device.
    EYE_HEIGHT_OFFSET = float("eye_offset", "Eye height (m)", 0.0, -0.5, 0.5, 0.02),
    /// Live override for the flat runtimes' projection FOV, in degrees. `0`
    /// (the default) means "no override - use `Game::desired_fov_deg()`";
    /// any positive value forces that FOV instead, for verifying the
    /// game-driven FOV seam (issue #1088) without a rebuild. `oculus_runtime`
    /// is untouched: OpenXR view FOVs must be used as-is. (`debug_runtime`
    /// shares its single flat projection between flat and `--vr` mode, so this
    /// override reaches both there.)
    FOV_OVERRIDE_DEG = float("fov_override_deg", "FOV override", 0.0, 0.0, 120.0, 1.0),
    /// Ceiling on how fast a physically simulated held melee weapon may be
    /// driven onto its tracked-hand target, in world units per second.
    ///
    /// The drive asks for exactly the velocity that lands the weapon on the
    /// hand this step, so in ordinary use the clamp never binds - a very fast
    /// human swing is well under it. It exists for the discontinuous case: a
    /// teleport or a restore can put the target a whole level away, and
    /// without a ceiling that becomes one enormous impulse into the geometry.
    /// Read before every physics step, so a headset can tune it without a
    /// rebuild.
    MELEE_MAX_SPEED = float("melee_max_speed", "Melee speed", 60.0, 1.0, 200.0, 5.0),
    /// The angular counterpart of [`MELEE_MAX_SPEED`], in radians per second.
    MELEE_MAX_TURN = float("melee_max_turn", "Melee turn", 60.0, 1.0, 200.0, 5.0),
    /// Minimum closing speed, in world units per second, at which a held melee
    /// weapon damages what it touches. This is the shipped VR melee rule: a
    /// swing damages because it was moving, not because a button was down,
    /// with the value acting as the swing/graze threshold so a weapon resting
    /// against a creature does nothing.
    ///
    /// The default is measured on the swept drive rather than guessed
    /// (`physics::held_melee_drive::free_swing_speed_separation`): a brisk
    /// swing peaks at **4.07** and ordinary walking carries the weapon at
    /// **1.80**, so 2.5 sits in the gap between them.
    ///
    /// `0` is a legacy escape hatch back to the trigger-held attack window
    /// (`TriggeredMeleeWeapon`'s trigger rule).
    MELEE_FREE_SWING_SPEED = float("melee_free_swing", "Free swing", 2.5, 0.0, 20.0, 0.5),
    /// Draw the tracked-hand glove *in addition to* a wielded weapon's own
    /// first-person model, instead of letting the weapon model stand in for the
    /// hand. `0` off, `1` on.
    ///
    /// This is the melee grip calibration instrument: the `_h` melee rigs bake
    /// their own arm and fist, and nothing else in the game shows where that
    /// baked fist lands relative to where the controller actually is. With both
    /// drawn at once the offset is directly visible - and directly tunable,
    /// since the registry reaches a headset without a rebuild.
    ///
    /// A 0/1 float because the registry has no `Bool` kind yet; it should
    /// become one when that lands.
    MELEE_GLOVE_OVERLAY = float("melee_glove_overlay", "Show hands", 0.0, 0.0, 1.0, 1.0),
    /// Draw the live contact volume of every held melee weapon as a world-space
    /// wireframe. `0` off, `1` on.
    ///
    /// The companion to [`MELEE_GLOVE_OVERLAY`]: that one answers "is the hand
    /// where my hand is", this one answers "is the damage volume where the
    /// weapon is". Read out of Rapier at the collider's own isometry, so it
    /// shows what is actually simulated rather than what the wield intended.
    MELEE_VOLUMES = float("melee_volumes", "Show hurtbox", 0.0, 0.0, 1.0, 1.0),
    /// Uniform scale applied to a wielded melee `_h` view model, about the
    /// baked fist so the grip stays on the controller.
    ///
    /// The 25AE first-person models are authored for a flat camera's own
    /// projection, where an oversized weapon reads better; VR draws them at
    /// true world scale, where the same exaggeration is simply a giant weapon.
    /// Measured on the shipped rigs (logged at every wield): the baked
    /// hand+forearm is ~57 cm against a real ~45 cm, and the weapons run
    /// 66-94 cm - a Wrench whose head sits 62 cm out of the fist.
    ///
    /// Unlike the rest of this table the value is read at *wield* time rather
    /// than every frame, because the correction is baked into the posed model
    /// once. It therefore takes effect on the next grab - one gesture in a
    /// headset, which is what this knob exists to serve - rather than
    /// instantly.
    ///
    /// The default is the balance point between the two, not a fit to either:
    /// the arm and the weapon are exaggerated by *different* factors, so no
    /// single scale makes both right. 0.7 draws a 46 cm Wrench on a 40 cm arm;
    /// making the arm exactly life-size (0.79) would leave the Wrench at 52,
    /// and making the Wrench right (~0.5) would leave a child's arm.
    MELEE_WIELD_SCALE = float("melee_scale", "Melee scale", 0.7, 0.25, 1.5, 0.05),
    /// Enables the detached debug ("free") camera. This is the *gate*, not
    /// the camera's own on/off: while it is false the toggle input is not
    /// even read, so a stray `Alt+V` (or controller chord) during normal play
    /// cannot detach the view. Turning it back off while detached also
    /// re-attaches the camera, so the switch is always a way out.
    FREE_CAMERA = bool("free_camera", "Free camera", false),
    /// Which pose the visibility engine culls from while the free camera is
    /// detached. Off (the default) culls from the *player*, so flying out
    /// shows exactly what the player's viewpoint decided - cells the player
    /// cannot see are genuinely absent, which is the point of the tool. On
    /// culls from the free camera instead, for when you just want to look at
    /// something. Inert while the camera is attached (the two poses are the
    /// same pose).
    FREE_CAMERA_CULL_FROM_CAMERA = bool("free_camera_cull", "Cull from cam", false),
    /// How fast the free camera flies, in the player's own speed units - the
    /// default IS [`PLAYER_MOVE_SPEED`], so "walking pace" cannot drift from
    /// what walking actually is. These are pre-scale SS2 units, not world
    /// units per second: the consumer divides by `dark::SCALE_FACTOR` (2.5)
    /// exactly as the player's locomotion does, so the default 25 travels 10
    /// world units a second, not 25. The range spans a crawl to a fast survey of a
    /// deck; live-tunable because the right speed depends entirely on what is
    /// being inspected.
    ///
    /// [`PLAYER_MOVE_SPEED`]: crate::mission::PLAYER_MOVE_SPEED
    FREE_CAMERA_SPEED = float(
        "free_camera_speed",
        "Cam speed",
        crate::mission::PLAYER_MOVE_SPEED,
        5.0,
        200.0,
        5.0
    ),
}

/// Every parameter with its id, in declaration order.
pub fn all() -> impl Iterator<Item = (DevParamId, &'static DevParam)> {
    PARAMS.iter().enumerate().map(|(i, p)| (DevParamId(i), p))
}

/// The static description of one parameter.
pub fn spec(id: DevParamId) -> &'static DevParam {
    &PARAMS[id.0]
}

/// Look a parameter up by its stable string key.
pub fn find(key: &str) -> Option<DevParamId> {
    PARAMS.iter().position(|p| p.key == key).map(DevParamId)
}

/// The parameter's current value.
pub fn get(id: DevParamId) -> f32 {
    f32::from_bits(VALUES[id.0].load(Ordering::Relaxed))
}

/// A [`DevParamKind::Bool`] parameter's current value. Any non-zero reading
/// is true, so this is also correct for a value written before the registry
/// normalized it.
pub fn get_bool(id: DevParamId) -> bool {
    get(id) != 0.0
}

/// Set a parameter, clamped into its range and snapped to its step grid.
/// Returns the value actually applied. A non-finite request is refused
/// (the current value is returned unchanged) so no consumer can ever read
/// a NaN out of the registry.
pub fn set(id: DevParamId, value: f32) -> f32 {
    let applied = apply(&spec(id).kind, get(id), value);
    VALUES[id.0].store(applied.to_bits(), Ordering::Relaxed);
    applied
}

/// The pure constrain step behind [`set`]: what `value` becomes under `kind`
/// when the parameter currently reads `current`. Split out so the clamp/snap
/// rules are testable without touching the process-global values (mutating
/// them from unit tests would race every parallel test that reads a live
/// parameter).
fn apply(kind: &DevParamKind, current: f32, value: f32) -> f32 {
    if !value.is_finite() {
        return current;
    }
    match *kind {
        DevParamKind::Bool => {
            if value == 0.0 {
                0.0
            } else {
                1.0
            }
        }
        DevParamKind::Float { min, max, step } => {
            if step > 0.0 {
                // Clamp the grid *index*, not the snapped value: clamping
                // afterwards would return `max` itself, which is off the
                // advertised grid whenever the range is not a whole number
                // of steps (and a future menu's arrows would then walk a
                // shifted grid).
                let max_index = ((max - min) / step).floor();
                let index = ((value - min) / step).round().clamp(0.0, max_index);
                (min + index * step).clamp(min, max)
            } else {
                value.clamp(min, max)
            }
        }
    }
}

/// Restore a parameter to its declared default, exactly. This stores the
/// default's own bits rather than routing through [`set`], whose snap grid
/// does not round-trip every default (e.g. 0.72 on a 0.02 grid lands on
/// 0.71999997 in f32) - the difference matters to anything that compares
/// against the default with `==` (the HTTP mirror's reset, a future menu
/// "reset" row).
pub fn reset(id: DevParamId) {
    VALUES[id.0].store(spec(id).default.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // No test here (or anywhere in the crate) mutates the live registry:
    // the values are process globals read by parallel tests all over the
    // crate, so a unit-test `set` would be a cross-test race. The constrain
    // rules are tested through the pure [`apply`]; that `set`/`reset` really
    // move what consumers render is proven end-to-end by the SDK e2e
    // (`dev-params.e2e.test.ts`), which watches the VR panel move over HTTP.

    /// The registry replaced two consts; anything but these exact defaults
    /// is a behavior change at launch.
    #[test]
    fn defaults_equal_the_consts_they_replaced() {
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
        assert_eq!(get(MELEE_MAX_SPEED), 60.0);
        assert_eq!(get(MELEE_MAX_TURN), 60.0);
    }

    /// Every declared default must be finite and inside its own range, or
    /// the seeded value would already violate the registry's contract.
    #[test]
    fn every_default_is_within_its_declared_range() {
        for (_, param) in all() {
            assert!(param.default.is_finite());
            match param.kind {
                DevParamKind::Float { min, max, step } => {
                    assert!(
                        (min..=max).contains(&param.default),
                        "{}: default {} outside {min}..={max}",
                        param.key,
                        param.default
                    );
                    assert!(step >= 0.0, "{}: negative step", param.key);
                }
                DevParamKind::Bool => assert!(
                    param.default == 0.0 || param.default == 1.0,
                    "{}: bool default {} is neither 0 nor 1",
                    param.key,
                    param.default
                ),
            }
        }
    }

    const GRID: DevParamKind = DevParamKind::Float {
        min: 0.5,
        max: 6.0,
        step: 0.1,
    };

    #[test]
    fn apply_passes_an_in_range_on_grid_value_through() {
        assert!((apply(&GRID, 2.0, 2.5) - 2.5).abs() < 1e-4);
    }

    #[test]
    fn apply_clamps_into_the_declared_range() {
        assert_eq!(apply(&GRID, 2.0, 99.0), 6.0);
        assert_eq!(apply(&GRID, 2.0, -3.0), 0.5);
    }

    #[test]
    fn apply_snaps_to_the_step_grid() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        // 0.013 is between grid points 0.0 and 0.02.
        let applied = apply(&kind, 0.72, 0.013);
        assert!((applied - 0.02).abs() < 1e-4, "got {applied}");
    }

    /// A range that is not a whole number of steps must still land on the
    /// grid at its top end: snapping *then* clamping would return `max`
    /// itself, off the advertised grid.
    #[test]
    fn apply_keeps_an_uneven_ranges_top_end_on_grid() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 0.05,
            step: 0.02,
        };
        assert!((apply(&kind, 0.0, 99.0) - 0.04).abs() < 1e-6);
    }

    /// Why [`reset`] stores the default's own bits instead of routing
    /// through the snap: the grid cannot represent every default exactly.
    #[test]
    fn the_snap_grid_does_not_round_trip_every_default() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        assert_ne!(apply(&kind, 0.72, 0.72).to_bits(), 0.72f32.to_bits());
    }

    #[test]
    fn a_non_finite_value_is_refused() {
        assert_eq!(apply(&GRID, 2.0, f32::NAN), 2.0);
        assert_eq!(apply(&GRID, 2.0, f32::INFINITY), 2.0);
        assert_eq!(apply(&GRID, 2.0, f32::NEG_INFINITY), 2.0);
    }

    /// A bool normalizes to exactly 0.0/1.0, so no consumer can read a third
    /// state out of the registry (and `get_bool`'s `!= 0.0` can never see a
    /// value the panel would then format inconsistently).
    #[test]
    fn apply_normalizes_a_bool_to_zero_or_one() {
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 1.0), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 1.0, 0.0), 0.0);
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 0.5), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 0.0, -3.0), 1.0);
    }

    #[test]
    fn a_non_finite_bool_is_refused() {
        assert_eq!(apply(&DevParamKind::Bool, 1.0, f32::NAN), 1.0);
    }

    /// The free camera is a debug tool: it must be off until asked for.
    #[test]
    fn the_free_camera_gate_defaults_off() {
        assert!(!get_bool(FREE_CAMERA));
        assert!(!get_bool(FREE_CAMERA_CULL_FROM_CAMERA));
    }

    #[test]
    fn unknown_keys_do_not_resolve() {
        assert_eq!(find("no_such_param"), None);
        assert_eq!(find("panel_distance"), Some(FRONTEND_PANEL_DISTANCE));
    }
}
