//! The cyber interface's entry/exit ramp: a single eased 0..1 value driving
//! every "the world becomes UI now" cue - the rim vignette (both
//! presentations), the VR comfort dim's strength, and the flat FOV pull -
//! instead of each snapping on with the panel.
//!
//! [`RampParams`] packages *how strong* and *how long*, so a future caller
//! (the plan's `ReadLastUnreadLog` shortcut, which wants a softer, shorter
//! ramp straight into the log reader) only has to build a different
//! `RampParams` rather than touch [`EntryExitRamp`] itself.

/// How a ramp attacks/releases and what it drives at full strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RampParams {
    /// Seconds to go from closed to fully open.
    pub attack_secs: f32,
    /// Seconds to go from open back to closed.
    pub release_secs: f32,
    /// Peak rim-vignette intensity (0..1) at full ramp.
    pub vignette_peak: f32,
    /// Flat FOV pulled in by this many degrees at full ramp (VR ignores it).
    pub fov_pull_deg: f32,
}

/// The deliberate cyber-interface open: a real "jacking in" moment.
pub const DEFAULT_ENTRY_EXIT: RampParams = RampParams {
    attack_secs: 0.35,
    release_secs: 0.25,
    vignette_peak: 0.45,
    fov_pull_deg: 6.0,
};

/// The rim tint blended in for the cyber interface - a cool cyan rather than
/// [`crate::hit_feedback`]'s arterial red, so the two read as different
/// things when they land on screen together.
pub const VIGNETTE_COLOR: cgmath::Vector3<f32> = cgmath::Vector3 {
    x: 0.05,
    y: 0.35,
    z: 0.55,
};

/// A single-target, eased attack/release envelope.
///
/// `progress` chases `target` (0 or 1) linearly at a rate set by whichever of
/// `attack_secs`/`release_secs` is currently relevant, and [`Self::eased`]
/// smoothsteps it for consumers. Re-targeting mid-ramp (open while closing,
/// or vice versa) just reverses the chase from wherever `progress` already
/// is - there is no snap.
#[derive(Debug, Clone, Copy)]
pub struct EntryExitRamp {
    params: RampParams,
    progress: f32,
    target: f32,
}

impl Default for EntryExitRamp {
    fn default() -> Self {
        Self::new()
    }
}

impl EntryExitRamp {
    pub fn new() -> Self {
        Self {
            params: DEFAULT_ENTRY_EXIT,
            progress: 0.0,
            target: 0.0,
        }
    }

    /// Start (or continue) opening, adopting `params` for this ramp - so a
    /// re-open while still closing can pick different timing/peaks than the
    /// ramp it interrupts.
    pub fn open(&mut self, params: RampParams) {
        self.params = params;
        self.target = 1.0;
    }

    /// Start (or continue) closing, at the same `params` the ramp last opened
    /// with.
    pub fn close(&mut self) {
        self.target = 0.0;
    }

    /// Close instantly, skipping the release entirely - for a takeover
    /// rather than a graceful exit (the pause menu forcing the interface
    /// shut via `Effect::CloseUseMode`). Without this the release keeps
    /// `update` un-advanced while the scene is suspended for the pause, so
    /// the vignette/dim/FOV pull would otherwise hang at whatever strength
    /// they were at for the entire pause.
    pub fn snap_closed(&mut self) {
        self.progress = 0.0;
        self.target = 0.0;
    }

    /// Advance by `delta_time_secs`. Negative or non-finite deltas are
    /// treated as no time passing, so a bad caller can't push `progress`
    /// outside 0..1 or send it the wrong way.
    pub fn update(&mut self, delta_time_secs: f32) {
        if !delta_time_secs.is_finite() || delta_time_secs <= 0.0 {
            return;
        }
        let duration = if self.target > self.progress {
            self.params.attack_secs
        } else {
            self.params.release_secs
        };
        let step = delta_time_secs / duration.max(1e-4);
        self.progress = if self.target >= self.progress {
            (self.progress + step).min(self.target)
        } else {
            (self.progress - step).max(self.target)
        }
        .clamp(0.0, 1.0);
    }

    /// Raw linear progress, 0 (closed) .. 1 (open).
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Smoothstepped progress - what every visual consumer reads, so the ramp
    /// eases rather than moves at a constant rate.
    pub fn eased(&self) -> f32 {
        let t = self.progress;
        t * t * (3.0 - 2.0 * t)
    }

    /// Peak rim-vignette intensity scaled by the current ramp.
    pub fn vignette_intensity(&self) -> f32 {
        self.eased() * self.params.vignette_peak
    }

    /// Flat FOV pull (degrees, to subtract from the base FOV) scaled by the
    /// current ramp.
    pub fn fov_pull_deg(&self) -> f32 {
        self.eased() * self.params.fov_pull_deg
    }

    /// True once the ramp has fully released and nothing is targeting open -
    /// the point at which a renderer can stop drawing anything for it.
    pub fn is_settled_closed(&self) -> bool {
        self.target <= 0.0 && self.progress <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: RampParams = RampParams {
        attack_secs: 1.0,
        release_secs: 0.5,
        vignette_peak: 0.5,
        fov_pull_deg: 10.0,
    };

    /// Negative test: an untouched ramp must already read as closed and
    /// settled, or every other test below is meaningless.
    #[test]
    fn a_fresh_ramp_is_closed_and_settled() {
        let ramp = EntryExitRamp::new();
        assert_eq!(ramp.progress(), 0.0);
        assert_eq!(ramp.eased(), 0.0);
        assert_eq!(ramp.vignette_intensity(), 0.0);
        assert_eq!(ramp.fov_pull_deg(), 0.0);
        assert!(ramp.is_settled_closed());
    }

    #[test]
    fn opening_ramps_up_over_the_attack_time_and_not_before() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        assert!(!ramp.is_settled_closed());

        // Halfway through the attack, progress is partway open (linearly),
        // strictly short of fully open.
        ramp.update(FAST.attack_secs * 0.5);
        assert!(ramp.progress() > 0.0 && ramp.progress() < 1.0);
        assert!((ramp.progress() - 0.5).abs() < 1e-4);

        // And it reaches (and clamps at) fully open, not past it.
        ramp.update(FAST.attack_secs);
        assert_eq!(ramp.progress(), 1.0);
        ramp.update(10.0);
        assert_eq!(ramp.progress(), 1.0);
    }

    #[test]
    fn closing_releases_over_the_release_time_then_settles() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs);
        assert_eq!(ramp.progress(), 1.0);

        ramp.close();
        assert!(!ramp.is_settled_closed(), "still ramping down");
        ramp.update(FAST.release_secs * 0.5);
        assert!(ramp.progress() > 0.0 && ramp.progress() < 1.0);

        ramp.update(FAST.release_secs);
        assert_eq!(ramp.progress(), 0.0);
        assert!(ramp.is_settled_closed());
    }

    /// Re-entry mid-ramp: opening again while still closing must reverse
    /// smoothly from wherever the ramp already is, never snap back to 0 first.
    #[test]
    fn reopening_mid_close_reverses_from_where_it_is() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs);
        ramp.close();
        ramp.update(FAST.release_secs * 0.5);
        let before = ramp.progress();
        assert!(before > 0.0 && before < 1.0);

        ramp.open(FAST);
        // No time has passed yet - re-targeting alone must not move progress.
        assert_eq!(ramp.progress(), before);
        assert!(!ramp.is_settled_closed());

        ramp.update(0.01);
        assert!(
            ramp.progress() > before,
            "reopening must move progress back up, not leave it falling"
        );
    }

    /// Symmetric case: closing mid-open reverses from wherever it is too.
    #[test]
    fn closing_mid_open_reverses_from_where_it_is() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs * 0.5);
        let before = ramp.progress();

        ramp.close();
        assert_eq!(ramp.progress(), before);
        ramp.update(0.01);
        assert!(ramp.progress() < before);
    }

    /// A takeover (`CloseUseMode`) must not leave the ramp mid-release for a
    /// scene that has stopped calling `update` (the pause menu suspends the
    /// scene) - `snap_closed` has to reach 0 immediately, not just retarget.
    #[test]
    fn snap_closed_settles_immediately_even_mid_open() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs * 0.5);
        assert!(ramp.progress() > 0.0);

        ramp.snap_closed();
        assert_eq!(ramp.progress(), 0.0);
        assert_eq!(ramp.eased(), 0.0);
        assert!(ramp.is_settled_closed());

        // And a caller that never calls `update` again (the exact pause
        // scenario) must not see it drift back open on its own.
        ramp.update(10.0);
        assert!(ramp.is_settled_closed());
    }

    #[test]
    fn a_non_finite_or_negative_delta_does_nothing() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs * 0.5);
        let before = ramp.progress();

        ramp.update(f32::NAN);
        assert_eq!(ramp.progress(), before);
        ramp.update(-1.0);
        assert_eq!(ramp.progress(), before);
        ramp.update(0.0);
        assert_eq!(ramp.progress(), before);
    }

    #[test]
    fn vignette_and_fov_scale_with_the_eased_ramp_and_its_own_params() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs);
        assert_eq!(ramp.eased(), 1.0);
        assert_eq!(ramp.vignette_intensity(), FAST.vignette_peak);
        assert_eq!(ramp.fov_pull_deg(), FAST.fov_pull_deg);
    }

    /// The eased curve must actually ease (sit at or above the linear ramp
    /// through the middle of the attack) rather than just proxy `progress`
    /// straight through - otherwise `eased()` is a pointless rename.
    #[test]
    fn eased_is_a_real_smoothstep_not_a_passthrough() {
        let mut ramp = EntryExitRamp::new();
        ramp.open(FAST);
        ramp.update(FAST.attack_secs * 0.25);
        assert!(ramp.eased() < ramp.progress());
    }
}
