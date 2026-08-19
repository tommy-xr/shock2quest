//! Runtime-togglable debug draws.
//!
//! The owner's ask is that debug overlays be switchable *while the game runs*
//! rather than requiring a rebuild - on the Quest, a redeploy. The eventual
//! home for these is the Developer screen's dev-params registry, but that
//! registry is float-only today (PR #1027, still open) and has no UI yet, so
//! stacking on it would entangle two open PRs without actually giving the
//! owner a switch. Instead this is a tiny process-global `AtomicBool` in the
//! *same shape* as the dev-params storage (one atomic per knob, read per
//! frame by its consumer, writable from any thread without `&mut Game`), so
//! migrating it to a `DevParamKind::Bool` once #1027 lands is a rename rather
//! than a redesign. See #1044.
//!
//! It is flipped by the `DebugToggleMeleeVolumes` input action, which reaches
//! it from a desktop key binding, from a Quest controller binding, and from
//! `POST /v1/input/action` on the debug runtime - none of which needs a
//! rebuild. That is deliberately the *only* channel: an
//! `experimental_features` flag would be a launch-time decision, which is
//! exactly the rebuild-and-redeploy cycle this exists to avoid.

use std::sync::atomic::{AtomicBool, Ordering};

/// Draw the melee contact volume and its per-frame swept path in world space.
static MELEE_VOLUMES: AtomicBool = AtomicBool::new(false);

/// Whether the melee contact/hurt volume overlay is currently drawn.
pub fn melee_volumes() -> bool {
    MELEE_VOLUMES.load(Ordering::Relaxed)
}

pub fn set_melee_volumes(enabled: bool) {
    MELEE_VOLUMES.store(enabled, Ordering::Relaxed);
}

/// Flip the overlay and report the new state (for the input action).
pub fn toggle_melee_volumes() -> bool {
    !MELEE_VOLUMES.fetch_xor(true, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The toggle is process-global, so this test owns it start-to-finish and
    /// restores the default rather than leaving it flipped for other tests.
    #[test]
    fn toggle_flips_and_reports_the_new_state() {
        let original = melee_volumes();

        set_melee_volumes(false);
        assert!(!melee_volumes());
        assert!(toggle_melee_volumes(), "toggling off->on reports on");
        assert!(melee_volumes());
        assert!(!toggle_melee_volumes(), "toggling on->off reports off");
        assert!(!melee_volumes());

        set_melee_volumes(original);
    }
}
