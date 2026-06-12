# Keybinding System Refactor

## Project Status: ✅ IMPLEMENTED (Phase 5 - Oculus mapper - pending)

Phases 1-4 and 6 are complete: the `Command` trait is gone, all runtimes pass
`InputActionState` to `Game::update`, the desktop runtime uses
`DesktopInputMapper`, and the debug runtime can inject actions via
`POST /v1/input/action` (verified end-to-end against medsci1.mis). The
oculus runtime passes an empty action state until an `OculusInputMapper`
is added (Phase 5).

A unified input action system that centralizes action definitions in `shock2vr`, allowing runtimes to map platform-specific inputs to shared actions. This enables the debug runtime to simulate any action via HTTP.

**Related:** [Debug Runtime](debug-runtime.md) - Primary consumer of this system

## Problem Statement

Currently, input handling is scattered across runtimes:

```
desktop_runtime/main.rs:618  → if window.get_key(Key::P) → PathfindingTestCommand
desktop_runtime/main.rs:596  → if window.get_key(Key::S) && alt_pressed → SaveCommand
oculus_runtime/lib.rs        → No command creation, reads InputContext values directly
debug_runtime/main.rs        → Cannot trigger PathfindingTest (architectural blocker)
```

**Issues:**
1. **Duplication**: Each runtime reimplements input → action mapping
2. **Inconsistency**: VR has no way to trigger debug actions
3. **Debug blocked**: HTTP API can't easily inject gameplay actions
4. **Hardcoded**: No way to remap keys or configure bindings

## Proposed Architecture

### Layer 1: Action Definitions (shock2vr)

**Key Design Decision: Discrete vs Continuous**

| Input Type | Examples | Handling |
|------------|----------|----------|
| **Discrete** | P key, A button, menu | `InputAction` enum - edge triggered |
| **Continuous** | Trigger squeeze, thumbstick | `InputContext` (existing) - read each frame |

For VR, "fire" is **continuous** (trigger value 0.0-1.0). Game logic in `VirtualHand` reads `trigger > 0.5` each frame via `InputContext` and sends messages to held entities. Non-contextual actions like `PathfindingTestCycle` or `QuickSave` are **discrete** - handled by `InputAction`.

```rust
// shock2vr/src/input/actions.rs

/// Discrete actions triggered by button presses (edge-triggered)
///
/// NOTE: Only non-contextual actions belong here. Hand interactions
/// (trigger pull, grab, use, drop) are contextual - they depend on
/// game state (what's held, what's nearby) and are handled by
/// VirtualHand which reads InputContext directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputAction {
    // Debug actions
    PathfindingTestCycle,
    PathfindingTestReset,

    // System actions
    QuickSave,
    QuickLoad,

    // Debug/dev actions
    SpawnDebugItem,
    MoveInventory,

    // Player state toggles
    ToggleCrouch,
    ToggleFlashlight,

    // UI actions
    OpenMenu,
    CloseMenu,
}
```

### What's NOT in InputAction

Hand interactions stay in `VirtualHand` (reads `InputContext` directly):

| Interaction | Why it's contextual |
|-------------|---------------------|
| TriggerPull | Sends message to *held entity* (game state) |
| Grab | Depends on what's nearby (physics query) |
| Drop | Only if holding something (game state) |
| Use | Depends on what you're pointing at (raycast) |

These need game state to determine the *target* of the action, so they can't be abstract `InputAction` variants.

### Relationship to InputContext

**InputContext stays unchanged.** It already has continuous values (trigger, thumbstick, grip). No need for a separate `InputAxis` enum.

```rust
// InputContext (existing - no changes needed)
pub struct InputContext {
    pub head: Head,
    pub left_hand: Hand,
    pub right_hand: Hand,
}

pub struct Hand {
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub thumbstick: Vector2<f32>,   // Continuous: movement
    pub trigger_value: f32,          // Continuous: fire
    pub squeeze_value: f32,          // Continuous: grab
    pub a_value: f32,                // Continuous: button (though often used as discrete)
}
```

**Summary:**
- **InputContext**: Poses + continuous values (existing, unchanged)
- **InputActionState**: Discrete triggered actions only (new)
- **No InputAxis**: Redundant - InputContext already has continuous values

Debug runtime:
- `/v1/control/input` → set continuous values (existing)
- `/v1/input/action` → trigger discrete actions (new)

### Layer 2: Action State (shock2vr)

Track discrete actions triggered this frame:

```rust
// shock2vr/src/input/state.rs

/// Discrete actions triggered this frame
#[derive(Default, Clone)]
pub struct InputActionState {
    /// Actions triggered this frame (just pressed)
    triggered: HashSet<InputAction>,

    /// Actions currently held (for hold-to-activate patterns)
    held: HashSet<InputAction>,
}

impl InputActionState {
    /// Check if action was just triggered (rising edge)
    pub fn just_triggered(&self, action: InputAction) -> bool {
        self.triggered.contains(&action)
    }

    /// Check if action is currently held
    pub fn is_held(&self, action: InputAction) -> bool {
        self.held.contains(&action)
    }

    /// Trigger an action (for runtime/debug injection)
    pub fn trigger(&mut self, action: InputAction) {
        self.triggered.insert(action);
        self.held.insert(action);
    }

    /// Clear triggered actions (call at end of frame)
    pub fn clear_triggered(&mut self) {
        self.triggered.clear();
    }

    /// Release a held action
    pub fn release(&mut self, action: InputAction) {
        self.held.remove(&action);
    }
}
```

### Layer 3: Action → Effect Mapping (shock2vr)

**Key insight: We can eliminate the `Command` trait entirely.**

Looking at current commands:
| Command | Uses `world`? | Uses input context? | Purpose |
|---------|---------------|---------------------|---------|
| `SaveCommand` | ❌ | ❌ | Returns `Effect::GlobalEffect(Save)` |
| `LoadCommand` | ❌ | ❌ | Returns `Effect::GlobalEffect(Load)` |
| `PathfindingTestCommand` | ❌ | ❌ | Returns `Effect::PathfindingTest` |
| `TransitionLevelCommand` | ❌ | ❌ | **Unused** (no call sites) — delete |
| `SpawnItemCommand` | ✅ | ✅ (head rotation) | Reads player pos → `Effect::CreateEntity` |
| `MoveInventoryCommand` | ✅ | ✅ (head rotation) | Reads player pos → `Effect::PositionInventory` |

4 of 6 commands are trivial mappers (one of which is dead code). The other 2 read
player position from the world **and** head rotation from the runtime's
`InputContext`. Player *body* rotation lives in `PlayerInfo`, but head rotation
does not exist in the world — so the dispatcher takes `&InputContext` and embeds
`head_rotation` in the emitted effects. The effect handler (which has world
access) resolves the player position.

```rust
// shock2vr/src/input/dispatcher.rs

pub struct ActionDispatcher;

impl ActionDispatcher {
    /// Convert triggered actions directly into effects (no Command indirection)
    pub fn dispatch(state: &InputActionState, input_context: &InputContext) -> Vec<Effect> {
        let mut effects = Vec::new();

        if state.just_triggered(InputAction::PathfindingTestCycle) {
            effects.push(Effect::PathfindingTest);
        }
        if state.just_triggered(InputAction::QuickSave) {
            effects.push(Effect::GlobalEffect(GlobalEffect::Save {
                file_name: "save1.sav".to_string(),
            }));
        }
        if state.just_triggered(InputAction::SpawnDebugItem) {
            // Head rotation comes from input context; effect handler
            // resolves player position from the world.
            effects.push(Effect::SpawnInFrontOfPlayer {
                template_id: -17, // Pistol
                head_rotation: input_context.head.rotation,
            });
        }
        // ... other mappings

        effects
    }
}
```

**Why this is better:**
- Removes `Command` trait indirection
- Effect handlers already have world access
- `InputAction` can be trivially serialized for HTTP
- Single enum (`Effect`) instead of two abstractions

### Layer 4: Runtime Input Mappers

Each runtime maps platform inputs to actions.

**Note on polling vs events**: The desktop runtime polls key state every frame
via `window.get_key()` inside `process_events()` (it does not consume
`WindowEvent::Key` events for these bindings), edge-detecting manually through
the `InputState` struct. The mapper therefore works on polled state and tracks
previous-frame key state internally to trigger actions on rising edges only.

```rust
// Example: desktop_runtime input mapper (polling-based)

pub struct DesktopInputMapper {
    key_bindings: HashMap<Key, InputAction>,
    modifier_bindings: HashMap<(Key, Modifier), InputAction>,
    /// Keys that were down last frame (for edge detection)
    prev_down: HashSet<(Key, bool)>, // (key, alt_held)
}

impl DesktopInputMapper {
    pub fn default() -> Self {
        let mut key_bindings = HashMap::new();
        key_bindings.insert(Key::P, InputAction::PathfindingTestCycle);
        key_bindings.insert(Key::I, InputAction::MoveInventory);
        key_bindings.insert(Key::Space, InputAction::SpawnDebugItem);

        let mut modifier_bindings = HashMap::new();
        modifier_bindings.insert((Key::S, Modifier::Alt), InputAction::QuickSave);
        modifier_bindings.insert((Key::L, Modifier::Alt), InputAction::QuickLoad);

        Self { key_bindings, modifier_bindings, prev_down: HashSet::new() }
    }

    /// Call once per frame from process_events(). Polls bound keys,
    /// triggers actions on rising edges.
    pub fn poll(&mut self, window: &Window, state: &mut InputActionState) {
        // For each binding: check window.get_key(key) == Action::Press
        // (plus modifier state), compare against prev_down, and call
        // state.trigger(action) only on the rising edge.
    }
}
```

**Behavior change (intentional)**: the current `I` key (MoveInventory) has no
debounce and fires every frame while held. Migrating to edge-triggered actions
fixes this — it will fire once per press.

### Layer 5: Debug Runtime Integration

Debug runtime can inject actions directly:

```rust
// debug_runtime HTTP endpoint

/// POST /v1/input/action
/// Body: { "action": "PathfindingTestCycle" }
async fn trigger_action(
    State(tx): State<Sender<RuntimeCommand>>,
    Json(request): Json<TriggerActionRequest>,
) -> Json<ActionResult> {
    let action = InputAction::from_str(&request.action)?;
    tx.send(RuntimeCommand::TriggerAction(action)).await?;
    Json(ActionResult { success: true })
}

// In game loop, action is injected into InputActionState
RuntimeCommand::TriggerAction(action) => {
    action_state.trigger(action);
}
```

## Data Flow Comparison

### Before (Current)
```
Desktop:  GLFW Key → process_events() → Command → game.update() → Effect
Oculus:   OpenXR → InputContext only (no commands)
Debug:    HTTP → ??? (blocked)
```

### After (Proposed)
```
Desktop:  GLFW Key → DesktopInputMapper → InputActionState → ActionDispatcher → Effects
Oculus:   OpenXR → OculusInputMapper → InputActionState → ActionDispatcher → Effects
Debug:    HTTP /v1/input/action → InputActionState → ActionDispatcher → Effects
```

**Eliminated layer:** `Command` trait is removed entirely. `InputAction` maps directly to `Effect`.

## Implementation Plan

### Phase 1: Core Types ✅
- [ ] Create `shock2vr/src/input/mod.rs` module
- [ ] Define `InputAction` enum with serde support (non-contextual actions only)
- [ ] Implement `InputActionState` struct
- [ ] Add unit tests for state management

### Phase 2: Action Dispatcher + Effect Changes ✅
- [ ] Create `ActionDispatcher::dispatch(state, input_context)` → `Vec<Effect>`
- [ ] Add new `Effect` variants for player-relative actions (carry head rotation
      from input context, since it isn't stored in the world):
  - `Effect::SpawnInFrontOfPlayer { template_id, head_rotation }`
  - `Effect::PositionInventoryRelativeToPlayer { head_rotation }`
- [ ] Add effect handlers in `mission_core.rs` that read player position
- [ ] Update `Game::update()` signature: remove `commands: Vec<Box<dyn Command>>`,
      pass `&InputActionState` instead, call dispatcher internally
- [ ] Update all 3 call sites atomically (desktop, debug ×2, oculus) — only
      3 call sites exist, so no dual-path migration needed

### Phase 3: Desktop Migration ✅
- [ ] Create `DesktopInputMapper` with current key bindings (polling-based)
- [ ] Refactor `process_events()` to populate `InputActionState`
- [ ] Remove inline command creation (no more `Box::new(PathfindingTestCommand)`)
- [ ] Remove unused `Vec<Effect>` return from `process_events()` (currently
      discarded as `_effects` at the call site)
- [ ] Verify existing keybinds work identically (exception: `I` key gains
      debounce — see behavior change note above)

### Phase 4: Debug Runtime Integration ✅
- [ ] Add `RuntimeCommand::TriggerAction(InputAction)`
- [ ] Implement `/v1/input/action` HTTP endpoint
- [ ] Implement `/v1/input/actions` GET endpoint (list available actions)
- [ ] Implement the existing `RuntimeCommand::PathfindingTest` stub
      (debug_runtime/src/main.rs — currently returns "requires keybinding
      system refactor") by funneling through action injection
- [ ] Test pathfinding via HTTP: `curl -X POST .../v1/input/action -d '{"action":"PathfindingTestCycle"}'`

### Phase 5: Oculus Runtime 🔲 PENDING
- [ ] Create `OculusInputMapper` for VR controllers
- [ ] Map A/B/X/Y buttons to appropriate actions
- [ ] Verify VR controls work correctly

### Phase 6: Deprecate Command Module ✅
- [ ] Remove `shock2vr/src/command/` module entirely
- [ ] Remove `Command` trait
- [ ] Update any remaining references
- [ ] Update CLAUDE.md with new architecture

## File Structure

```
shock2vr/src/
├── input/
│   ├── mod.rs           # Module exports
│   ├── actions.rs       # InputAction enum (with serde)
│   ├── state.rs         # InputActionState
│   └── dispatcher.rs    # ActionDispatcher → Effects
├── command/             # DEPRECATED - to be removed in Phase 6
│   └── ...
├── lib.rs               # Updated Game::update() signature
└── ...

runtimes/
├── desktop_runtime/src/
│   ├── main.rs          # Simplified, uses mapper
│   └── input_mapper.rs  # DesktopInputMapper
├── oculus_runtime/src/
│   ├── lib.rs           # Uses mapper
│   └── input_mapper.rs  # OculusInputMapper
└── debug_runtime/src/
    └── main.rs          # TriggerAction via HTTP
```

## API Examples

### Desktop Key Handling (After)
```rust
// In process_events() - runtime populates action state
for (_, event) in glfw::flush_messages(&events) {
    if let WindowEvent::Key(key, _, Action::Press, modifiers) = event {
        input_mapper.process_key(key, modifiers, &mut action_state);
    }
}

// In main loop - game.update() handles dispatch internally
game.update(&time, &input_context, &action_state);  // No more Vec<Command>!
```

### Game::update() Signature Change
```rust
// Before:
pub fn update(&mut self, time: &Time, input: &InputContext, commands: Vec<Box<dyn Command>>)

// After:
pub fn update(&mut self, time: &Time, input: &InputContext, actions: &InputActionState)
```

### Debug Runtime HTTP (After)
```bash
# Trigger discrete action
curl -X POST http://127.0.0.1:8080/v1/input/action \
  -H "Content-Type: application/json" \
  -d '{"action": "PathfindingTestCycle"}'

curl -X POST http://127.0.0.1:8080/v1/input/action \
  -H "Content-Type: application/json" \
  -d '{"action": "QuickSave"}'

# Set continuous values (existing endpoint - for hand interactions)
curl -X POST http://127.0.0.1:8080/v1/control/input \
  -H "Content-Type: application/json" \
  -d '{"channel": "right_hand.trigger_value", "value": 1.0}'

# List available actions
curl http://127.0.0.1:8080/v1/input/actions
```

### VR Controller Mapping (After)
```rust
impl OculusInputMapper {
    pub fn process_controller(&self, state: &ControllerState, action_state: &mut InputActionState) {
        // Map controller buttons to discrete actions
        if state.menu_button_just_pressed {
            action_state.trigger(InputAction::OpenMenu);
        }

        // Debug: Map Y button to spawn item (dev builds only)
        if state.y_button_just_pressed {
            action_state.trigger(InputAction::SpawnDebugItem);
        }

        // NOTE: Continuous values (trigger, grip, thumbstick) go into InputContext
        // and are handled by VirtualHand for contextual interactions.
        // InputActionState is only for discrete, non-contextual actions.
    }
}
```

## Migration Strategy

1. **Additive first**: Add new input module without breaking existing code
2. **Atomic signature change**: `Game::update()` has only 3 call sites
   (desktop, debug ×2, oculus passes `vec![]`) — change them all in one PR
   rather than maintaining a dual path
3. **Remove deprecated**: Delete the `command/` module (including the unused
   `TransitionLevelCommand`) once runtimes are migrated

## Benefits

| Benefit | Description |
|---------|-------------|
| **Centralized** | All action definitions in one place |
| **Testable** | InputActionState can be unit tested |
| **Debug-friendly** | HTTP can trigger any action |
| **Consistent** | Same actions available on all platforms |
| **Configurable** | Easy path to keybinding configuration |
| **Extensible** | Adding new actions is trivial |

## Future Extensions

1. **Config file**: Load keybindings from TOML/JSON
2. **Rebinding UI**: In-game key remapping
3. **Action combos**: Multi-button combinations
4. **Hold vs Press**: Different behavior for tap vs hold
5. **Input recording**: Record/replay input sequences for testing

## Related Documents

- `projects/debug-runtime.md` - Debug runtime that will use this system
- `CLAUDE.md` - Development guidelines
- `shock2vr/src/command/mod.rs` - Existing Command trait
