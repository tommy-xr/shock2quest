//! Shared alertness system for AI entities.
//!
//! This module provides common infrastructure for managing AI alertness levels,
//! including state tracking, timing-based escalation/decay, and ECS synchronization.
//!
//! # Alertness Levels
//! - `Lowest` (0): Unaware, idle state
//! - `Low` (1): Slightly alerted, searching
//! - `Moderate` (2): Actively tracking target
//! - `High` (3): Full combat alert
//!
//! # Usage
//! ```ignore
//! let mut state = AlertnessState::new(AIAlertLevel::Lowest);
//! let timings = AlertnessTimings::from_aware_delay(&prop_ai_aware_delay);
//! let cap = prop_ai_alert_cap;
//!
//! // Each frame:
//! if let Some((old, new)) = process_alertness_update(&mut state, is_visible, delta, &timings, &cap) {
//!     // Handle level change (play speech, change model, etc.)
//! }
//! ```

use dark::properties::{AIAlertLevel, PropAIAlertCap, PropAIAwareDelay};
use num_traits::{FromPrimitive, ToPrimitive};
use shipyard::EntityId;

use crate::scripts::{AIPropertyUpdate, Effect};

/// Tracks the current alertness state for an AI entity.
#[derive(Clone, Debug)]
pub struct AlertnessState {
    /// Current alertness level
    pub current_level: AIAlertLevel,
    /// Peak alertness level reached (used for min_relax clamping)
    pub peak_level: AIAlertLevel,
    /// Time the player has been continuously visible (seconds)
    pub visible_time: f32,
    /// Time the player has been continuously hidden (seconds)
    pub hidden_time: f32,
    /// Time since last level change (seconds)
    pub time_since_level_change: f32,
}

impl AlertnessState {
    /// Create a new alertness state at the given initial level.
    pub fn new(initial_level: AIAlertLevel) -> Self {
        Self {
            current_level: initial_level,
            peak_level: initial_level,
            visible_time: 0.0,
            hidden_time: 0.0,
            time_since_level_change: 0.0,
        }
    }

    /// Reset timers when transitioning to a new level.
    pub fn reset_timers_for_level_change(&mut self) {
        self.time_since_level_change = 0.0;
    }
}

impl Default for AlertnessState {
    fn default() -> Self {
        Self::new(AIAlertLevel::Lowest)
    }
}

/// Timing configuration for alertness escalation and decay.
///
/// All times are in seconds. Derived from `PropAIAwareDelay` which stores
/// times in milliseconds.
#[derive(Clone, Debug)]
pub struct AlertnessTimings {
    /// Time to escalate from Lowest -> Low (derived: to_two / 2)
    pub to_low: f32,
    /// Time to escalate from Low -> Moderate (derived: to_two / 2)
    pub to_moderate: f32,
    /// Time to escalate from Moderate -> High (from to_three)
    pub to_high: f32,
    /// Time to decay from High -> Moderate (from three_reuse)
    pub from_high: f32,
    /// Time to decay from Moderate -> Low (from two_reuse)
    pub from_moderate: f32,
    /// Time to decay from Low -> Lowest (from ignore_range)
    pub from_low: f32,
}

impl AlertnessTimings {
    /// Create timings from a PropAIAwareDelay property.
    ///
    /// The Dark Engine property uses:
    /// - `to_two`: time to reach level 2 (Moderate) - we split this for Low and Moderate
    /// - `to_three`: time to reach level 3 (High)
    /// - `two_reuse`: decay time from level 2
    /// - `three_reuse`: decay time from level 3
    /// - `ignore_range`: time to return to Lowest
    pub fn from_aware_delay(delay: &PropAIAwareDelay) -> Self {
        let to_two_secs = delay.to_two as f32 / 1000.0;
        Self {
            // Split to_two between Lowest->Low and Low->Moderate transitions
            to_low: to_two_secs / 2.0,
            to_moderate: to_two_secs / 2.0,
            to_high: delay.to_three as f32 / 1000.0,
            from_high: delay.three_reuse as f32 / 1000.0,
            from_moderate: delay.two_reuse as f32 / 1000.0,
            from_low: delay.ignore_range as f32 / 1000.0,
        }
    }
}

/// Default timing constants (in seconds) matching camera_ai.rs defaults.
const DEFAULT_ESCALATE_SECONDS: f32 = 3.0;
const DEFAULT_DECAY_SECONDS: f32 = 5.0;

impl Default for AlertnessTimings {
    fn default() -> Self {
        Self {
            to_low: DEFAULT_ESCALATE_SECONDS / 2.0,
            to_moderate: DEFAULT_ESCALATE_SECONDS / 2.0,
            to_high: DEFAULT_ESCALATE_SECONDS,
            from_high: DEFAULT_DECAY_SECONDS,
            from_moderate: DEFAULT_DECAY_SECONDS,
            from_low: DEFAULT_DECAY_SECONDS,
        }
    }
}

/// Process visibility and update alertness state.
///
/// Returns `Some((old_level, new_level))` if a level change occurred, `None` otherwise.
///
/// # Arguments
/// * `state` - The current alertness state (will be mutated)
/// * `is_visible` - Whether the target (player) is currently visible
/// * `delta` - Time elapsed since last update (seconds)
/// * `timings` - Timing configuration for escalation/decay
/// * `alert_cap` - Level constraints (min/max/relax floor)
pub fn process_alertness_update(
    state: &mut AlertnessState,
    is_visible: bool,
    delta: f32,
    timings: &AlertnessTimings,
    alert_cap: &PropAIAlertCap,
) -> Option<(AIAlertLevel, AIAlertLevel)> {
    state.time_since_level_change += delta;

    if is_visible {
        state.visible_time += delta;
        state.hidden_time = 0.0;
        try_escalate(state, timings, alert_cap)
    } else {
        state.hidden_time += delta;
        state.visible_time = 0.0;
        try_decay(state, timings, alert_cap)
    }
}

/// Attempt to escalate alertness level based on visibility time.
fn try_escalate(
    state: &mut AlertnessState,
    timings: &AlertnessTimings,
    alert_cap: &PropAIAlertCap,
) -> Option<(AIAlertLevel, AIAlertLevel)> {
    let (threshold, next_level) = match state.current_level {
        AIAlertLevel::Lowest => (timings.to_low, AIAlertLevel::Low),
        AIAlertLevel::Low => (timings.to_moderate, AIAlertLevel::Moderate),
        AIAlertLevel::Moderate => (timings.to_high, AIAlertLevel::High),
        AIAlertLevel::High => return None, // Already at max
    };

    if state.visible_time >= threshold {
        let old_level = state.current_level;
        if set_level(state, next_level, alert_cap) {
            state.visible_time = 0.0;
            return Some((old_level, state.current_level));
        }
    }
    None
}

/// Attempt to decay alertness level based on hidden time.
fn try_decay(
    state: &mut AlertnessState,
    timings: &AlertnessTimings,
    alert_cap: &PropAIAlertCap,
) -> Option<(AIAlertLevel, AIAlertLevel)> {
    let (threshold, next_level) = match state.current_level {
        AIAlertLevel::High => (timings.from_high, AIAlertLevel::Moderate),
        AIAlertLevel::Moderate => (timings.from_moderate, AIAlertLevel::Low),
        AIAlertLevel::Low => (timings.from_low, AIAlertLevel::Lowest),
        AIAlertLevel::Lowest => return None, // Already at min
    };

    if state.hidden_time >= threshold {
        let old_level = state.current_level;
        if set_level(state, next_level, alert_cap) {
            state.hidden_time = 0.0;
            return Some((old_level, state.current_level));
        }
    }
    None
}

/// Set alertness level with clamping. Returns true if level actually changed.
///
/// Also updates peak level tracking:
/// - If new level > peak, peak is updated to new level
/// - If new level < peak, peak is clamped to max(new_level, min_relax)
pub fn set_level(
    state: &mut AlertnessState,
    new_level: AIAlertLevel,
    alert_cap: &PropAIAlertCap,
) -> bool {
    let clamped = clamp_level(new_level, alert_cap);
    if clamped == state.current_level {
        return false;
    }

    state.current_level = clamped;
    state.reset_timers_for_level_change();

    // Update peak level
    if level_to_u32(clamped) > level_to_u32(state.peak_level) {
        state.peak_level = clamped;
    } else if level_to_u32(clamped) < level_to_u32(state.peak_level) {
        // When decaying, peak can't go below min_relax
        state.peak_level = max_level(clamped, alert_cap.min_relax);
    }

    true
}

/// Clamp level to alert cap constraints (min_level to max_level).
pub fn clamp_level(level: AIAlertLevel, cap: &PropAIAlertCap) -> AIAlertLevel {
    let raw = level_to_u32(level);
    let min = level_to_u32(cap.min_level);
    let max = level_to_u32(cap.max_level);
    let clamped = raw.clamp(min, max);
    AIAlertLevel::from_u32(clamped).unwrap_or(cap.max_level)
}

/// Create an Effect to sync alertness state to the ECS.
pub fn sync_alertness_effect(entity_id: EntityId, state: &AlertnessState) -> Effect {
    Effect::SetAIProperty {
        entity_id,
        update: AIPropertyUpdate::Alertness {
            level: state.current_level,
            peak: state.peak_level,
        },
    }
}

/// Convert AIAlertLevel to u32 for comparison.
fn level_to_u32(level: AIAlertLevel) -> u32 {
    level.to_u32().unwrap_or(0)
}

/// Return the higher of two alertness levels.
fn max_level(a: AIAlertLevel, b: AIAlertLevel) -> AIAlertLevel {
    if level_to_u32(a) >= level_to_u32(b) {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cap() -> PropAIAlertCap {
        PropAIAlertCap {
            max_level: AIAlertLevel::High,
            min_level: AIAlertLevel::Lowest,
            min_relax: AIAlertLevel::Low,
        }
    }

    fn fast_timings() -> AlertnessTimings {
        AlertnessTimings {
            to_low: 1.0,
            to_moderate: 1.0,
            to_high: 1.0,
            from_high: 1.0,
            from_moderate: 1.0,
            from_low: 1.0,
        }
    }

    #[test]
    fn test_escalation_through_all_levels() {
        let mut state = AlertnessState::new(AIAlertLevel::Lowest);
        let timings = fast_timings();
        let cap = default_cap();

        // Lowest -> Low
        let result = process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Lowest, AIAlertLevel::Low)));
        assert_eq!(state.current_level, AIAlertLevel::Low);

        // Low -> Moderate
        let result = process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Low, AIAlertLevel::Moderate)));
        assert_eq!(state.current_level, AIAlertLevel::Moderate);

        // Moderate -> High
        let result = process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Moderate, AIAlertLevel::High)));
        assert_eq!(state.current_level, AIAlertLevel::High);

        // Already at High, no change
        let result = process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(result, None);
        assert_eq!(state.current_level, AIAlertLevel::High);
    }

    #[test]
    fn test_decay_through_all_levels() {
        let mut state = AlertnessState::new(AIAlertLevel::High);
        state.peak_level = AIAlertLevel::High;
        let timings = fast_timings();
        let cap = default_cap();

        // High -> Moderate
        let result = process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::High, AIAlertLevel::Moderate)));
        assert_eq!(state.current_level, AIAlertLevel::Moderate);

        // Moderate -> Low
        let result = process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Moderate, AIAlertLevel::Low)));
        assert_eq!(state.current_level, AIAlertLevel::Low);

        // Low -> Lowest
        let result = process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Low, AIAlertLevel::Lowest)));
        assert_eq!(state.current_level, AIAlertLevel::Lowest);

        // Already at Lowest, no change
        let result = process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(result, None);
        assert_eq!(state.current_level, AIAlertLevel::Lowest);
    }

    #[test]
    fn test_alert_cap_max_level() {
        let mut state = AlertnessState::new(AIAlertLevel::Lowest);
        let timings = fast_timings();
        let cap = PropAIAlertCap {
            max_level: AIAlertLevel::Moderate, // Capped at Moderate
            min_level: AIAlertLevel::Lowest,
            min_relax: AIAlertLevel::Low,
        };

        // Escalate to Low
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Low);

        // Escalate to Moderate (cap)
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);

        // Try to escalate to High - should stay at Moderate due to cap
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);
    }

    #[test]
    fn test_alert_cap_min_level() {
        let mut state = AlertnessState::new(AIAlertLevel::High);
        state.peak_level = AIAlertLevel::High;
        let timings = fast_timings();
        let cap = PropAIAlertCap {
            max_level: AIAlertLevel::High,
            min_level: AIAlertLevel::Moderate, // Cannot go below Moderate
            min_relax: AIAlertLevel::Low,
        };

        // Decay to Moderate
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);

        // Try to decay to Low - should stay at Moderate due to min_level
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);
    }

    #[test]
    fn test_peak_level_tracking() {
        let mut state = AlertnessState::new(AIAlertLevel::Lowest);
        let timings = fast_timings();
        let cap = default_cap();

        assert_eq!(state.peak_level, AIAlertLevel::Lowest);

        // Escalate to Low
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.peak_level, AIAlertLevel::Low);

        // Escalate to Moderate
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.peak_level, AIAlertLevel::Moderate);

        // Escalate to High
        process_alertness_update(&mut state, true, 1.0, &timings, &cap);
        assert_eq!(state.peak_level, AIAlertLevel::High);

        // Decay to Moderate - peak should stay at High (or clamp to min_relax)
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);
        // Peak tracks current when decaying, but clamped to min_relax (Low)
        assert_eq!(state.peak_level, AIAlertLevel::Moderate);
    }

    #[test]
    fn test_peak_level_min_relax() {
        let mut state = AlertnessState::new(AIAlertLevel::High);
        state.peak_level = AIAlertLevel::High;
        let timings = fast_timings();
        let cap = PropAIAlertCap {
            max_level: AIAlertLevel::High,
            min_level: AIAlertLevel::Lowest,
            min_relax: AIAlertLevel::Moderate, // Peak can't go below Moderate
        };

        // Decay to Moderate
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Moderate);
        assert_eq!(state.peak_level, AIAlertLevel::Moderate);

        // Decay to Low
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Low);
        // Peak should stay at Moderate due to min_relax
        assert_eq!(state.peak_level, AIAlertLevel::Moderate);

        // Decay to Lowest
        process_alertness_update(&mut state, false, 1.0, &timings, &cap);
        assert_eq!(state.current_level, AIAlertLevel::Lowest);
        // Peak still at Moderate
        assert_eq!(state.peak_level, AIAlertLevel::Moderate);
    }

    #[test]
    fn test_no_change_without_enough_time() {
        let mut state = AlertnessState::new(AIAlertLevel::Lowest);
        let timings = fast_timings(); // 1.0 second thresholds
        let cap = default_cap();

        // Not enough time to escalate
        let result = process_alertness_update(&mut state, true, 0.5, &timings, &cap);
        assert_eq!(result, None);
        assert_eq!(state.current_level, AIAlertLevel::Lowest);
        assert_eq!(state.visible_time, 0.5);

        // Accumulate more time
        let result = process_alertness_update(&mut state, true, 0.5, &timings, &cap);
        assert_eq!(result, Some((AIAlertLevel::Lowest, AIAlertLevel::Low)));
        assert_eq!(state.current_level, AIAlertLevel::Low);
    }

    #[test]
    fn test_visibility_resets_hidden_time() {
        let mut state = AlertnessState::new(AIAlertLevel::High);
        state.peak_level = AIAlertLevel::High;
        let timings = fast_timings();
        let cap = default_cap();

        // Start hidden
        process_alertness_update(&mut state, false, 0.5, &timings, &cap);
        assert_eq!(state.hidden_time, 0.5);

        // Become visible - hidden time should reset
        process_alertness_update(&mut state, true, 0.1, &timings, &cap);
        assert_eq!(state.hidden_time, 0.0);
        assert_eq!(state.visible_time, 0.1);
    }

    #[test]
    fn test_from_aware_delay() {
        let delay = PropAIAwareDelay {
            to_two: 4000,    // 4 seconds -> split to 2s each for to_low and to_moderate
            to_three: 3000,  // 3 seconds
            two_reuse: 5000, // 5 seconds
            three_reuse: 6000,
            ignore_range: 7000,
        };

        let timings = AlertnessTimings::from_aware_delay(&delay);

        assert_eq!(timings.to_low, 2.0);
        assert_eq!(timings.to_moderate, 2.0);
        assert_eq!(timings.to_high, 3.0);
        assert_eq!(timings.from_high, 6.0);
        assert_eq!(timings.from_moderate, 5.0);
        assert_eq!(timings.from_low, 7.0);
    }

    #[test]
    fn test_clamp_level() {
        let cap = PropAIAlertCap {
            max_level: AIAlertLevel::Moderate,
            min_level: AIAlertLevel::Low,
            min_relax: AIAlertLevel::Low,
        };

        assert_eq!(clamp_level(AIAlertLevel::Lowest, &cap), AIAlertLevel::Low);
        assert_eq!(clamp_level(AIAlertLevel::Low, &cap), AIAlertLevel::Low);
        assert_eq!(
            clamp_level(AIAlertLevel::Moderate, &cap),
            AIAlertLevel::Moderate
        );
        assert_eq!(
            clamp_level(AIAlertLevel::High, &cap),
            AIAlertLevel::Moderate
        );
    }
}
