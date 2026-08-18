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
//! Ground rule: **if the consumer does not read the parameter every frame, it
//! does not belong in this table yet.** Values latched at construction time
//! (feature gates, collider dimensions, anything baked into a scene object)
//! would need a poke/rebuild path this registry deliberately does not have.

use once_cell::sync::Lazy;
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
    /// Extra eye height, in real-world meters, added on top of the stance's
    /// base eye height. Default 0 = the unmodified `PLAYER_EYE_HEIGHT`.
    EYE_HEIGHT_OFFSET = float("eye_offset", "Eye height offset (m)", 0.0, -0.5, 0.5, 0.02),
}

/// Current values, as `f32` bits. `Lazy` because `f32::to_bits` in a `static`
/// initializer wants a newer const story than the table needs; the first read
/// simply seeds every slot with its default.
static VALUES: Lazy<Vec<AtomicU32>> = Lazy::new(|| {
    PARAMS
        .iter()
        .map(|p| AtomicU32::new(p.default.to_bits()))
        .collect()
});

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

    /// The registry replaced three consts; anything but these exact defaults
    /// is a behavior change at launch.
    #[test]
    fn defaults_equal_the_consts_they_replaced() {
        let _guard = test_guard();
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
        assert_eq!(get(EYE_HEIGHT_OFFSET), 0.0);
    }

    // The mutation tests below deliberately touch only EYE_HEIGHT_OFFSET: it
    // has the fewest reader tests elsewhere in the crate, and each of those
    // takes [`test_guard`] too, so a temporarily non-default value is never
    // observed.

    #[test]
    fn set_moves_the_live_value_and_reports_what_it_applied() {
        let _guard = test_guard();
        // Negative first: before the set, the default is in effect.
        assert_eq!(get(EYE_HEIGHT_OFFSET), 0.0);
        let applied = set(EYE_HEIGHT_OFFSET, 0.2);
        assert!((applied - 0.2).abs() < 1e-4);
        assert!((get(EYE_HEIGHT_OFFSET) - 0.2).abs() < 1e-4);
        set(EYE_HEIGHT_OFFSET, spec(EYE_HEIGHT_OFFSET).default);
    }

    #[test]
    fn set_clamps_into_the_declared_range() {
        let _guard = test_guard();
        assert_eq!(set(EYE_HEIGHT_OFFSET, 99.0), 0.5);
        assert_eq!(set(EYE_HEIGHT_OFFSET, -3.0), -0.5);
        set(EYE_HEIGHT_OFFSET, spec(EYE_HEIGHT_OFFSET).default);
    }

    #[test]
    fn set_snaps_to_the_step_grid() {
        let _guard = test_guard();
        // 0.013 is between grid points 0.0 and 0.02 (min -0.5, step 0.02).
        let applied = set(EYE_HEIGHT_OFFSET, 0.013);
        assert!((applied - 0.02).abs() < 1e-4, "got {applied}");
        set(EYE_HEIGHT_OFFSET, spec(EYE_HEIGHT_OFFSET).default);
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
