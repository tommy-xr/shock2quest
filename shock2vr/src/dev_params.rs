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
    // `Bool` and `Enum(&'static [&'static str])` go here when a param needs
    // them - stored in the same f32 bits as 0.0/1.0 and the variant index.
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
macro_rules! dev_params {
    ($($(#[$doc:meta])* $id:ident = float($key:literal, $label:literal, $default:expr, $min:expr, $max:expr, $step:expr)),+ $(,)?) => {
        /// Every registered parameter, in declaration order (index == id).
        pub static PARAMS: &[DevParam] = &[
            $(DevParam {
                key: $key,
                label: $label,
                kind: DevParamKind::Float { min: $min, max: $max, step: $step },
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
    // An eye-height offset was considered for this seed set and deliberately
    // left out: on the Quest the eye is the *tracked pose* (capped in
    // `oculus_runtime`), which never reads `player_eye_height_for` - so the
    // knob would be inert exactly where live tuning matters, while the debug
    // runtime's `--vr` path (which does read it) would falsely "verify" it.
    // It returns with the Developer screen once the tracked-stage offset is
    // wired through the runtime (head and hands together).
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

/// Set a parameter, clamped into its range and snapped to its step grid.
/// Returns the value actually applied. A non-finite request is refused
/// (the current value is returned unchanged) so no consumer can ever read
/// a NaN out of the registry.
pub fn set(id: DevParamId, value: f32) -> f32 {
    if !value.is_finite() {
        return get(id);
    }
    let applied = match spec(id).kind {
        DevParamKind::Float { min, max, step } => {
            let snapped = if step > 0.0 {
                min + ((value - min) / step).round() * step
            } else {
                value
            };
            snapped.clamp(min, max)
        }
    };
    VALUES[id.0].store(applied.to_bits(), Ordering::Relaxed);
    applied
}

/// Restore a parameter to its declared default, exactly. This stores the
/// default's own bits rather than routing through [`set`], whose snap grid
/// does not round-trip every default (e.g. 0.72 on a 0.02 grid lands on
/// 0.71999997 in f32) - the difference matters to tests (and a future menu
/// "reset" row) that compare against the default with `==`.
pub fn reset(id: DevParamId) {
    VALUES[id.0].store(spec(id).default.to_bits(), Ordering::Relaxed);
}

/// Serializes tests that read or write the registry: the values are process
/// globals, so a mutation test running in parallel with a test that asserts a
/// default would flake. Writers must restore defaults before dropping the
/// guard.
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry replaced two consts; anything but these exact defaults
    /// is a behavior change at launch.
    #[test]
    fn defaults_equal_the_consts_they_replaced() {
        let _guard = test_guard();
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
    }

    // The mutation tests below hold [`test_guard`] and restore defaults
    // before releasing it; tests elsewhere in the crate that assert against a
    // parameter's live value take the same guard so a temporarily non-default
    // value is never observed.

    #[test]
    fn set_moves_the_live_value_and_reports_what_it_applied() {
        let _guard = test_guard();
        // Negative first: before the set, the default is in effect.
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        let applied = set(FRONTEND_PANEL_DISTANCE, 2.5);
        assert!((applied - 2.5).abs() < 1e-4);
        assert!((get(FRONTEND_PANEL_DISTANCE) - 2.5).abs() < 1e-4);
        reset(FRONTEND_PANEL_DISTANCE);
    }

    #[test]
    fn set_clamps_into_the_declared_range() {
        let _guard = test_guard();
        assert_eq!(set(FRONTEND_PANEL_DISTANCE, 99.0), 6.0);
        assert_eq!(set(FRONTEND_PANEL_DISTANCE, -3.0), 0.5);
        reset(FRONTEND_PANEL_DISTANCE);
    }

    #[test]
    fn set_snaps_to_the_step_grid() {
        let _guard = test_guard();
        // 0.013 is between grid points 0.0 and 0.02 (min 0.0, step 0.02).
        let applied = set(WORLD_DIM_STRENGTH, 0.013);
        assert!((applied - 0.02).abs() < 1e-4, "got {applied}");
        reset(WORLD_DIM_STRENGTH);
    }

    /// `set(default)` is NOT an exact restore (the snap grid can miss the
    /// default's bits); `reset` must be.
    #[test]
    fn reset_restores_the_exact_default_even_off_grid() {
        let _guard = test_guard();
        // 0.72 is default for a 0.02-step grid: set() snaps it to 0.71999997.
        set(WORLD_DIM_STRENGTH, spec(WORLD_DIM_STRENGTH).default);
        assert_ne!(get(WORLD_DIM_STRENGTH).to_bits(), 0.72f32.to_bits());
        reset(WORLD_DIM_STRENGTH);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
    }

    #[test]
    fn a_non_finite_set_is_refused() {
        let _guard = test_guard();
        assert_eq!(set(WORLD_DIM_STRENGTH, f32::NAN), 0.72);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
        assert_eq!(set(WORLD_DIM_STRENGTH, f32::INFINITY), 0.72);
    }

    #[test]
    fn unknown_keys_do_not_resolve() {
        assert_eq!(find("no_such_param"), None);
        assert_eq!(find("panel_distance"), Some(FRONTEND_PANEL_DISTANCE));
    }
}
