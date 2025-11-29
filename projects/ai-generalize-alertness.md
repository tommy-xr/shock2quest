# Generalized Alertness System for All AI Types

## Overview

This project aims to extract and generalize the alertness system currently implemented in `camera_ai.rs` to make it reusable across all AI types (cameras, turrets, animated monsters). The goal is to provide consistent alertness behavior while allowing each AI type to customize responses to alertness changes.

## Current State Analysis

### Camera AI (Fully Implemented)
- **Location**: `shock2vr/src/scripts/ai/camera_ai.rs`
- **Alertness Levels**: 4 levels defined in `dark/src/properties/prop_ai_alert_cap.rs:12-17`
  - `Lowest` (0), `Low` (1), `Moderate` (2), `High` (3)
- **State Transitions**: Based on visibility timers with configurable delays
- **Responses**: Model changes (green/yellow/red), speech lines, joint animation
- **Properties Used**:
  - `PropAIAlertness` - current and peak alertness levels
  - `PropAIAlertCap` - min/max level constraints and relax floor
  - `PropAIAwareDelay` - timing for escalation and decay
- **Known Bug**: Camera skips `Low` level during escalation (`Lowest → Moderate` instead of `Lowest → Low → Moderate`). Speech concepts exist for all levels (`tolevelone`, `toleveltwo`, `tolevelthree`), suggesting this was unintentional.

### Turret AI (Simple Binary)
- **Location**: `shock2vr/src/scripts/ai/turret_ai.rs`
- **Current Implementation**: Binary states (`Closed`, `Opening`, `Closing`, `Open`)
- **Visibility Detection**: Simple `is_player_visible` check with instant response
- **No Alertness Integration**: Opens immediately when player visible

### Animated Monster AI (Behavior-Based)
- **Location**: `shock2vr/src/scripts/ai/animated_monster_ai.rs`
- **Behavior Trait**: `shock2vr/src/scripts/ai/behavior/behavior.rs`
- **Current Behaviors**: `IdleBehavior`, `ChaseBehavior`, `MeleeAttackBehavior`, `RangedAttackBehavior`, `WanderBehavior`, `ScriptedSequenceBehavior`, `DeadBehavior`
- **No Alertness Levels**: Behaviors switch without gradual awareness
- **Could Benefit From**: Alertness-driven behavior transitions

## Design Decisions

### Naming Convention
**Decision**: Keep original Dark Engine property field names for compatibility with game data.
- `to_two` (not `to_moderate`) - time to reach level 2
- `to_three` (not `to_high`) - time to reach level 3
- `two_reuse` (not `from_moderate`) - decay time from level 2
- `three_reuse` (not `from_high`) - decay time from level 3
- `ignore_range` - time to return to Lowest

### Escalation Path
**Decision**: Support full 4-level escalation: `Lowest → Low → Moderate → High`
- Fix the camera bug that skips `Low` level
- Use `to_two` for `Lowest → Low` (half the original time)
- Use `to_two` for `Low → Moderate` (half the original time)
- Use `to_three` for `Moderate → High`

### Alert Propagation
**Decision**: Defer to Phase 5 (final phase).

## Proposed Architecture

### Core Components Location
Create `shock2vr/src/scripts/ai/alertness.rs` containing:

### 1. AlertnessState Struct
```rust
#[derive(Clone, Debug)]
pub struct AlertnessState {
    // Core alertness levels
    pub current_level: AIAlertLevel,
    pub peak_level: AIAlertLevel,

    // Timing tracking
    pub visible_time: f32,
    pub hidden_time: f32,
    pub time_since_level_change: f32,
}

impl AlertnessState {
    pub fn new(initial_level: AIAlertLevel) -> Self {
        Self {
            current_level: initial_level,
            peak_level: initial_level,
            visible_time: 0.0,
            hidden_time: 0.0,
            time_since_level_change: 0.0,
        }
    }

    pub fn reset_timers_for_level_change(&mut self) {
        self.time_since_level_change = 0.0;
    }
}
```

### 2. AlertnessTimings Struct
```rust
/// Timing configuration derived from PropAIAwareDelay
/// All times are in seconds (converted from milliseconds in the property)
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
    pub fn from_aware_delay(delay: &PropAIAwareDelay) -> Self {
        let to_two_secs = delay.to_two as f32 / 1000.0;
        Self {
            to_low: to_two_secs / 2.0,
            to_moderate: to_two_secs / 2.0,
            to_high: delay.to_three as f32 / 1000.0,
            from_high: delay.three_reuse as f32 / 1000.0,
            from_moderate: delay.two_reuse as f32 / 1000.0,
            from_low: delay.ignore_range as f32 / 1000.0,
        }
    }
}
```

### 3. Core Helper Functions
```rust
/// Process visibility and update alertness state
/// Returns Some(new_level) if a level change occurred, None otherwise
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

fn try_escalate(
    state: &mut AlertnessState,
    timings: &AlertnessTimings,
    alert_cap: &PropAIAlertCap,
) -> Option<(AIAlertLevel, AIAlertLevel)> {
    let (threshold, next_level) = match state.current_level {
        AIAlertLevel::Lowest => (timings.to_low, AIAlertLevel::Low),
        AIAlertLevel::Low => (timings.to_moderate, AIAlertLevel::Moderate),
        AIAlertLevel::Moderate => (timings.to_high, AIAlertLevel::High),
        AIAlertLevel::High => return None,
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

fn try_decay(
    state: &mut AlertnessState,
    timings: &AlertnessTimings,
    alert_cap: &PropAIAlertCap,
) -> Option<(AIAlertLevel, AIAlertLevel)> {
    let (threshold, next_level) = match state.current_level {
        AIAlertLevel::High => (timings.from_high, AIAlertLevel::Moderate),
        AIAlertLevel::Moderate => (timings.from_moderate, AIAlertLevel::Low),
        AIAlertLevel::Low => (timings.from_low, AIAlertLevel::Lowest),
        AIAlertLevel::Lowest => return None,
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

/// Set alertness level with clamping. Returns true if level changed.
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
        state.peak_level = max_level(clamped, alert_cap.min_relax);
    }

    true
}

/// Clamp level to alert cap constraints
pub fn clamp_level(level: AIAlertLevel, cap: &PropAIAlertCap) -> AIAlertLevel {
    let raw = level_to_u32(level);
    let min = level_to_u32(cap.min_level);
    let max = level_to_u32(cap.max_level);
    let clamped = raw.clamp(min, max);
    AIAlertLevel::from_u32(clamped).unwrap_or(cap.max_level)
}

/// Create Effect to sync alertness state to ECS
pub fn sync_alertness_effect(entity_id: EntityId, state: &AlertnessState) -> Effect {
    Effect::SetAIProperty {
        entity_id,
        update: AIPropertyUpdate::Alertness {
            level: state.current_level,
            peak: state.peak_level,
        },
    }
}

fn level_to_u32(level: AIAlertLevel) -> u32 {
    level.to_u32().unwrap_or(0)
}

fn max_level(a: AIAlertLevel, b: AIAlertLevel) -> AIAlertLevel {
    if level_to_u32(a) >= level_to_u32(b) { a } else { b }
}
```

## Implementation Phases

### Phase 1: Core Alertness Module ✅
**Goal**: Create shared alertness infrastructure

1. Create `shock2vr/src/scripts/ai/alertness.rs` with:
   - `AlertnessState` struct
   - `AlertnessTimings` struct
   - `process_alertness_update()` function
   - `clamp_level()`, `set_level()` helper functions
   - `sync_alertness_effect()` for ECS updates
2. Add module to `shock2vr/src/scripts/ai/mod.rs`
3. Add unit tests for state transitions:
   - Test escalation through all 4 levels
   - Test decay through all 4 levels
   - Test alert cap clamping (min/max/relax)
   - Test peak level tracking

### Phase 2: Camera AI Migration ✅
**Goal**: Refactor camera to use shared alertness module

1. Replace `CameraState` alertness fields with embedded `AlertnessState`
2. Replace `CameraTimings` with `AlertnessTimings::from_aware_delay()`
3. Replace `process_alertness()` with calls to `alertness::process_alertness_update()`
4. Keep camera-specific logic:
   - Model switching (`sync_model`)
   - Speech (`on_alert_level_changed`, `maybe_play_level_sustain`)
   - Joint animation
5. **Bug fix**: Camera now escalates through `Low` level
6. Test: Verify camera behavior with existing speech/model changes

### Phase 3: Turret AI Integration ✅
**Goal**: Add alertness tracking to turrets (optional behavioral changes)

1. Add `AlertnessState` to `TurretAI` struct
2. Load `PropAIAwareDelay` and `PropAIAlertCap` in initialize
3. Update alertness each frame via `process_alertness_update()`
4. **Preserve existing behavior**: Turret still opens immediately on visibility
5. Optional enhancements (can defer):
   - Targeting accuracy varies by alertness level
   - Play alert sounds on level changes
6. Test: Verify turret still functions, alertness tracked correctly

### Phase 4: Monster AI Integration ✅
**Goal**: Drive behavior selection from alertness

1. Add `AlertnessState` to `AnimatedMonsterAI` struct
2. Load `PropAIAwareDelay` and `PropAIAlertCap` in initialize
3. Update alertness each frame
4. Modify `next_behavior()` logic to consider alertness:
   - `Lowest` → `IdleBehavior` or `WanderBehavior`
   - `Low` → `WanderBehavior` (searching)
   - `Moderate` → `ChaseBehavior`
   - `High` → `RangedAttackBehavior` or `MeleeAttackBehavior`
5. Handle behavior transitions when alertness changes
6. Test: Verify monsters react more gradually to player visibility

### Phase 5: Polish and Alert Propagation ✅
**Goal**: Add propagation and debugging tools

1. Add debug visualization for alertness (similar to camera FOV debug)
2. Implement alert propagation via `AIAlertLink` or similar:
   - When AI reaches High alertness, notify linked AIs
   - Linked AIs escalate their alertness
3. Add shared alertness indicator UI (optional)
4. Performance review

## File Summary

| File | Changes |
|------|---------|
| `shock2vr/src/scripts/ai/alertness.rs` | **New** - Core alertness module with state machine and unit tests |
| `shock2vr/src/scripts/ai/ai_debug_util.rs` | **New** - Shared debug visualization for alertness bars and FOV cones |
| `shock2vr/src/scripts/ai/ai_util.rs` | Added `is_player_visible_in_fov()` with documented heading conventions |
| `shock2vr/src/scripts/ai/mod.rs` | Add `mod alertness`, `mod ai_debug_util` |
| `shock2vr/src/scripts/ai/camera_ai.rs` | Refactor to use alertness module, fix Low level bug, FOV-aware visibility |
| `shock2vr/src/scripts/ai/turret_ai.rs` | Add alertness tracking, FOV-aware visibility |
| `shock2vr/src/scripts/ai/animated_monster_ai.rs` | Add alertness-driven behavior selection, FOV-aware visibility |

## Success Criteria

- [x] Camera AI retains all existing functionality (speech, models, animation)
- [x] Camera AI now uses `Low` level correctly
- [x] Turret AI tracks alertness (even if behavior unchanged initially)
- [x] Monster AI behaviors driven by alertness levels
- [x] Shared alertness code in `alertness.rs` used by all AI types
- [x] Unit tests for alertness state machine
- [x] No performance regression

## Implementation Notes

### FOV-Aware Visibility Check
The `is_player_visible_in_fov()` function in `ai_util.rs` combines line-of-sight raycast with a field-of-view cone check. Each AI type has a fixed instantaneous FOV:
- **Camera**: 30° half-angle (60° total cone) - defined as `CAMERA_FOV_HALF_ANGLE`
- **Turret**: 30° half-angle (60° total cone) - defined as `TURRET_FOV_HALF_ANGLE`
- **Monster**: 60° half-angle (120° total cone) - defined as `MONSTER_FOV_HALF_ANGLE`

### Heading Convention for FOV Checks
The `is_player_visible_in_fov()` and `draw_debug_fov()` functions take a `heading` parameter that is applied on top of `pose.rotation`. Different entity types require different heading values due to how they manage rotation:

| Entity Type | Heading Parameter | Reason |
|-------------|-------------------|--------|
| **Monster** | `Deg(0.0)` | Rotation set via `Effect::SetRotation`, so `pose.rotation` already contains full orientation |
| **Camera** | `Deg(view_angle + 90.0)` | Rotation via joint transforms; +90 offset aligns with joint coordinate system |
| **Turret** | `-current_heading` | Similar to camera but negated due to turret joint rotation calculation |

### Camera Alertness Levels to Model Colors
- `Lowest` / `Low` → Green (camera in safe state)
- `Moderate` → Yellow (camera alerted)
- `High` → Red (camera fully alarmed)

The camera's default `min_relax` is set to `Lowest` to allow full decay back to green.

### Debug Visualization
Created `ai_debug_util.rs` with shared debug visualization utilities:
- `AlertnessDebugConfig` - configures alertness bar position/size per entity type
- `FovDebugConfig` - configures FOV cone visualization per entity type
- `draw_debug_alertness()` - draws alertness level bar and visibility indicator
- `draw_debug_fov()` - draws FOV cone with heading-based orientation

## Future Enhancements (Out of Scope)

1. **Environmental Factors**: Darkness/noise affect alertness detection
2. **Player Actions**: Stealth mechanics to reduce alertness
3. **Persistent Alertness**: Remember player across map transitions
4. **Difficulty Scaling**: Adjust timing based on difficulty setting
5. **Audio Cues**: Consistent audio feedback for alertness changes
