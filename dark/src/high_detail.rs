//! Whether to render the 25th Anniversary Edition's high-detail (`PMNM`) meshes.
//!
//! Model loading lives in this crate, which has no access to `shock2vr`'s
//! `GameOptions`, so the decision is published here as a process-wide setting
//! that `shock2vr` writes once at startup. That replaces the `SS2_PMNM_MESHES`
//! env var this began as - env vars are effectively unsettable in a Quest APK,
//! which made the feature impossible to control on the platform that most needs
//! to control it.
//!
//! Default: on, everywhere.
//!
//! - The upgraded creature meshes are ~7x the triangles of the originals and
//!   cost about 2x the draw calls, which desktop absorbs easily. It is also a
//!   no-op on a classic install: those assets carry no `PMNM` chunk at all
//!   (verified across all 1576 `.bin` files), so nothing changes for anyone
//!   not running the remaster's data.
//! - **Quest** shipped off pending a device measurement (#1022). Measured on a
//!   Quest 3 (release APK, 90 Hz): earth.mis and medsci1.mis hold the same
//!   FPS/stale-frame profile as with the gate closed (~0.3 ms extra combined
//!   eye time in a 6-creature stress scene, +12 MiB PSS on medsci1), so it is
//!   now on by default on device too. `no_high_detail_meshes` (see
//!   `Game::resolve_high_detail_meshes`) forces it off on runtimes that pass
//!   experimental flags (desktop/debug); the Quest runtime hardcodes its flag
//!   set, so disabling there is a rebuild.

use std::sync::atomic::{AtomicBool, Ordering};

/// See the module docs for the measurements behind this.
const DEFAULT_ENABLED: bool = true;

static ENABLED: AtomicBool = AtomicBool::new(DEFAULT_ENABLED);

/// The default, before any explicit override.
pub fn default_enabled() -> bool {
    DEFAULT_ENABLED
}

/// Set once at startup, before any model is loaded.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
