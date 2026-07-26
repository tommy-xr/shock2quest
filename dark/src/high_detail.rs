//! Whether to render the 25th Anniversary Edition's high-detail (`PMNM`) meshes.
//!
//! Model loading lives in this crate, which has no access to `shock2vr`'s
//! `GameOptions`, so the decision is published here as a process-wide setting
//! that `shock2vr` writes once at startup. That replaces the `SS2_PMNM_MESHES`
//! env var this began as - env vars are effectively unsettable in a Quest APK,
//! which made the feature impossible to control on the platform that most needs
//! to control it.
//!
//! Defaults are per-platform, because the tradeoff differs:
//!
//! - **Desktop: on.** The upgraded creature meshes are ~7x the triangles of the
//!   originals and cost about 2x the draw calls, which desktop absorbs easily.
//!   It is also a no-op on a classic install: those assets carry no `PMNM`
//!   chunk at all (verified across all 1576 `.bin` files), so nothing changes
//!   for anyone not running the remaster's data.
//! - **Android/Quest: off.** That same 7x/2x has not been measured on device.
//!   Turn it on explicitly once it has.

use std::sync::atomic::{AtomicBool, Ordering};

/// See the module docs for why these differ.
const fn platform_default() -> bool {
    !cfg!(target_os = "android")
}

static ENABLED: AtomicBool = AtomicBool::new(platform_default());

/// The default for this platform, before any explicit override.
pub fn default_enabled() -> bool {
    platform_default()
}

/// Set once at startup, before any model is loaded.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
