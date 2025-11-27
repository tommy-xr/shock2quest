# Generalized Alertness System for All AI Types

## Overview

This project aims to extract and generalize the alertness system currently implemented in `camera_ai.rs` to make it reusable across all AI types (cameras, turrets, animated monsters). The goal is to provide consistent alertness behavior while allowing each AI type to customize responses to alertness changes.

## Current State Analysis

### Camera AI (Fully Implemented)
- **Alertness Levels**: 4 levels (Lowest, Low, Moderate, High)
- **State Transitions**: Based on visibility timers with configurable delays
- **Responses**: Model changes (green/yellow/red), speech lines, animation adjustments
- **Properties Used**: PropAIAlertness, PropAIAlertCap, PropAIAwareDelay

### Turret AI (Simple Binary)
- **Current Implementation**: Binary states (Closed/Opening/Closing/Open)
- **Visibility Detection**: Simple is_player_visible check
- **No Alertness Integration**: Could benefit from gradual awareness

### Animated Monster AI (Behavior-Based)
- **Current Implementation**: Behavior selection (Idle, Chase, RangedAttack)
- **No Alertness Levels**: Behaviors switch without gradual awareness
- **Could Benefit From**: Alertness-driven behavior transitions

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

    // Optional: AI-specific state
    pub custom_data: Option<Box<dyn Any>>,
}

impl AlertnessState {
    pub fn new(initial_level: AIAlertLevel) -> Self {
        Self {
            current_level: initial_level,
            peak_level: initial_level,
            visible_time: 0.0,
            hidden_time: 0.0,
            time_since_level_change: 0.0,
            custom_data: None,
        }
    }

    pub fn reset_for_level(&mut self, level: AIAlertLevel) {
        self.time_since_level_change = 0.0;
        // AI-specific reset logic can be added via custom_data
    }
}
```

### 2. AlertnessConfig Struct
```rust
#[derive(Clone, Debug)]
pub struct AlertnessConfig {
    // Timing parameters (from PropAIAwareDelay)
    pub escalation_times: EscalationTimes,
    pub decay_times: DecayTimes,

    // Level constraints (from PropAIAlertCap)
    pub alert_cap: PropAIAlertCap,

    // AI-specific configuration
    pub response_config: AlertnessResponseConfig,
}

#[derive(Clone, Debug)]
pub struct EscalationTimes {
    pub to_low: f32,      // Time to go from Lowest -> Low (if applicable)
    pub to_moderate: f32, // Time to go from Low -> Moderate (was to_two)
    pub to_high: f32,     // Time to go from Moderate -> High (was to_three)
}

#[derive(Clone, Debug)]
pub struct DecayTimes {
    pub from_high: f32,     // Time to decay from High -> Moderate (was three_reuse)
    pub from_moderate: f32, // Time to decay from Moderate -> Low (was two_reuse)
    pub from_low: f32,      // Time to decay from Low -> Lowest (was ignore_range)
}

#[derive(Clone, Debug)]
pub enum AlertnessResponseConfig {
    Camera {
        models: CameraModels,
        speech_enabled: bool,
    },
    Turret {
        activation_delay: f32,
        targeting_accuracy_by_level: [f32; 4],
    },
    Monster {
        behavior_thresholds: BehaviorThresholds,
    },
}
```

### 3. AlertnessManager Trait
```rust
pub trait AlertnessManager {
    /// Update alertness based on visibility and elapsed time
    fn update_alertness(
        &mut self,
        entity_id: EntityId,
        is_visible: bool,
        delta: f32,
        config: &AlertnessConfig,
        effects: &mut Vec<Effect>,
    );

    /// Handle alertness level changes
    fn on_alertness_changed(
        &mut self,
        entity_id: EntityId,
        old_level: AIAlertLevel,
        new_level: AIAlertLevel,
        was_visible: bool,
        effects: &mut Vec<Effect>,
    );

    /// Get current alertness state
    fn get_alertness_state(&self) -> &AlertnessState;

    /// Set alertness level with proper clamping and effects
    fn set_alertness_level(
        &mut self,
        entity_id: EntityId,
        new_level: AIAlertLevel,
        config: &AlertnessConfig,
        effects: &mut Vec<Effect>,
    ) -> bool;
}
```

### 4. Common Alertness Logic
```rust
/// Standard implementation of alertness updates that can be reused
pub fn process_standard_alertness(
    state: &mut AlertnessState,
    entity_id: EntityId,
    is_visible: bool,
    delta: f32,
    config: &AlertnessConfig,
    effects: &mut Vec<Effect>,
    on_change: impl Fn(AIAlertLevel, AIAlertLevel, bool),
) {
    if is_visible {
        state.visible_time += delta;
        state.hidden_time = 0.0;

        // Handle escalation logic
        match state.current_level {
            AIAlertLevel::Lowest => {
                if state.visible_time >= config.escalation_times.to_moderate {
                    // Escalate to Moderate
                }
            }
            // ... other escalation cases
        }
    } else {
        state.hidden_time += delta;
        state.visible_time = 0.0;

        // Handle decay logic
        // ... decay cases
    }
}

/// Helper to clamp alertness levels based on caps
pub fn clamp_alertness_level(
    level: AIAlertLevel,
    cap: &PropAIAlertCap,
) -> AIAlertLevel {
    // Implementation from camera_ai
}

/// Helper to sync alertness with ECS
pub fn sync_alertness_property(
    entity_id: EntityId,
    state: &AlertnessState,
    effects: &mut Vec<Effect>,
) {
    effects.push(Effect::SetAIProperty {
        entity_id,
        update: AIPropertyUpdate::Alertness {
            level: state.current_level,
            peak: state.peak_level,
        },
    });
}
```

## Implementation Phases

### Phase 1: Core Alertness Module (Day 1)
1. Create `alertness.rs` with core structs and traits
2. Implement common helper functions
3. Add unit tests for alertness state transitions

### Phase 2: Camera AI Migration (Day 2)
1. Refactor `camera_ai.rs` to use AlertnessManager
2. Create CameraAlertnessManager implementing the trait
3. Ensure existing functionality is preserved
4. Test camera behavior remains unchanged

### Phase 3: Turret AI Integration (Day 3)
1. Create TurretAlertnessManager
2. Map turret states to alertness levels:
   - Closed → Lowest
   - Opening → Low/Moderate (based on timing)
   - Open → High
   - Closing → Moderate/Low (based on timing)
3. Add gradual awareness before opening
4. Improve targeting based on alertness level

### Phase 4: Monster AI Integration (Day 4-5)
1. Create MonsterAlertnessManager
2. Map behaviors to alertness levels:
   - Lowest → IdleBehavior or WanderBehavior
   - Low → SearchBehavior (new, looking for player)
   - Moderate → ChaseBehavior
   - High → RangedAttackBehavior/MeleeAttackBehavior
3. Add smooth behavior transitions
4. Consider alertness in animation selection

### Phase 5: Polish and Extensions (Day 6)
1. Add debug visualization for all AI alertness
2. Create shared alertness HUD indicators
3. Add configuration for alertness spread (one AI alerting others)
4. Performance optimization

## Benefits

1. **Consistency**: All AI types follow similar alertness patterns
2. **Reusability**: Core logic shared, reducing duplication
3. **Extensibility**: Easy to add alertness to new AI types
4. **Configurability**: Each AI type can customize responses
5. **Debugging**: Unified alertness visualization and logging
6. **Gameplay**: More predictable and fair AI awareness system

## Testing Strategy

1. **Unit Tests**: Test state transitions and timing logic
2. **Integration Tests**: Verify each AI type responds correctly
3. **Regression Tests**: Ensure camera AI behavior unchanged
4. **Performance Tests**: Verify no performance degradation
5. **Gameplay Tests**: Manual testing of AI reactions

## Future Enhancements

1. **Alert Propagation**: AIs can alert nearby allies
2. **Environmental Factors**: Darkness/noise affect alertness
3. **Player Actions**: Stealth mechanics to reduce alertness
4. **Persistent Alertness**: Remember player across map transitions
5. **Difficulty Scaling**: Adjust timing based on difficulty
6. **Audio Cues**: Consistent audio feedback for alertness changes

## Success Criteria

- [ ] Camera AI retains all existing functionality
- [ ] Turret AI gains gradual awareness
- [ ] Monster AI behaviors driven by alertness
- [ ] Code duplication reduced by >50%
- [ ] All AI types use consistent alertness visualization
- [ ] No performance regression
- [ ] Easy to add alertness to new AI scripts