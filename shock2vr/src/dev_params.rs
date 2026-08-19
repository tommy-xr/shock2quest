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
    /// of `step` from `min` (the grid the menu's `<`/`>` arrows walk).
    Float { min: f32, max: f32, step: f32 },
    /// An on/off switch, stored in the same f32 bits as 0.0/1.0 (the shape
    /// this module always anticipated). Any non-zero request stores exactly
    /// 1.0, so a consumer can compare with `== 1.0` and the menu's two arrows
    /// both just flip it.
    Bool,
    // `Enum(&'static [&'static str])` goes here when a param needs it -
    // stored in the same bits as the variant index.
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
macro_rules! dev_param_kind {
    (float($min:expr, $max:expr, $step:expr)) => {
        DevParamKind::Float {
            min: $min,
            max: $max,
            step: $step,
        }
    };
    (bool()) => {
        DevParamKind::Bool
    };
}

macro_rules! dev_params {
    ($($(#[$doc:meta])* $id:ident = $kind:ident($key:literal, $label:literal, $default:expr $(, $arg:expr)*)),+ $(,)?) => {
        /// Every registered parameter, in declaration order (index == id).
        pub static PARAMS: &[DevParam] = &[
            $(DevParam {
                key: $key,
                label: $label,
                kind: dev_param_kind!($kind($($arg),*)),
                default: $default,
            }),+
        ];

        /// Current values, as `f32` bits, seeded with the defaults. A sized
        /// array, not a `&[_]` slice: a borrow of interior-mutable data
        /// cannot be promoted to `'static` (E0492).
        static VALUES: [AtomicU32; [$(($default as f32)),+].len()] =
            [$(AtomicU32::new(($default as f32).to_bits())),+];

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
    /// Draw the contact volume of whatever is held as a world-space wireframe
    /// (see `mission::melee_debug`). `mission_core` reads it once per rendered
    /// frame, satisfying the registry's every-frame ground rule.
    MELEE_VOLUMES = bool("melee_volumes", "Melee volumes", 0.0),
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

/// A [`DevParamKind::Bool`] parameter's current value, as a bool. Consumers
/// of a switch read this rather than comparing floats at every call site.
pub fn get_bool(id: DevParamId) -> bool {
    debug_assert!(
        matches!(spec(id).kind, DevParamKind::Bool),
        "get_bool on a non-Bool param: {}",
        spec(id).key
    );
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

    /// A switch stores exactly 0.0/1.0 whatever it is handed, so consumers
    /// can compare it without a tolerance and the menu's two arrows agree.
    #[test]
    fn a_bool_param_normalizes_to_zero_or_one() {
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 1.0), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 1.0, 0.0), 0.0);
        // Anything non-zero is "on" - including values a float grid would
        // have snapped somewhere else entirely.
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 0.3), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 0.0, -2.0), 1.0);
        // ...and the registry-wide non-finite guard still wins.
        assert_eq!(apply(&DevParamKind::Bool, 1.0, f32::NAN), 1.0);
    }

    /// The overlay must default to off: this PR changes no visible behavior
    /// until someone turns it on.
    #[test]
    fn the_melee_volume_overlay_defaults_off() {
        assert!(!get_bool(MELEE_VOLUMES));
        assert!(matches!(spec(MELEE_VOLUMES).kind, DevParamKind::Bool));
    }

    /// The registry replaced two consts; anything but these exact defaults
    /// is a behavior change at launch.
    #[test]
    fn defaults_equal_the_consts_they_replaced() {
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
    }

    /// Every declared default must be finite and inside its own range, or
    /// the seeded value would already violate the registry's contract.
    #[test]
    fn every_default_is_within_its_declared_range() {
        for (_, param) in all() {
            assert!(param.default.is_finite(), "{}", param.key);
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
                // A switch's only legal defaults are the two it stores;
                // anything else would differ from what `set` can produce.
                DevParamKind::Bool => assert!(
                    param.default == 0.0 || param.default == 1.0,
                    "{}: bool default {} is neither off nor on",
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

    #[test]
    fn unknown_keys_do_not_resolve() {
        assert_eq!(find("no_such_param"), None);
        assert_eq!(find("panel_distance"), Some(FRONTEND_PANEL_DISTANCE));
    }
}
