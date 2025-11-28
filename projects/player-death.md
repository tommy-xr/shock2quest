# Player Death

When the player 'dies' in the VR game, we want to make them _feel_ it. The idea is to take control of the camera and have the camera 'collapse' to the floor.

If there is a revive chamber (a ResurrectionStation / ResStation - based on template -1677) and the following conditions are met:
1. The user has _activated_ the revive station
2. The user has 5 nanites

If the revive station is available, 5 nanites should be deducted from the player balance and the player should be teleported to the revive station.

## Death Animation

When the player dies, we should lerp the camera position to a position on the floor, close to straight downward, with a random vector facing front or straightforward.

## Engine Enhancements

The _biggest_ change to support this is the ability to have the game 'override' the VR head position. I'd propose adding a new API like 'CameraOverride { transition: 0.0, position, forward, up }`.

Then, we'd add a 'CameraOverride' to the 'SceneContext' as an Option. If not specified (Default), we use the VR position, rotation as today. If it _is_ specified, we'll override the camera position / forward / up vectors, with a transition parameter (how _much_ we are overriding).

## Other enhancements

A 'static' or 'blood' effect when the player dies would be pretty cool but not on the critical path.

---

## Implementation Plan

### Codebase Analysis Findings

**Current Health System**:
- Player health tracked via `PropHitPoints`/`PropMaxHitPoints` components
- Displayed on left arm HUD (`shock2vr/src/hud/virtual_arms.rs:115-136`)
- Death triggered by `MessagePayload::Damage` leading to `Effect::SlayEntity`

**Death Handling**:
- `SlayEntity` effect processed in `mission_core.rs:751`
- Currently removes entities via `slay_entity()` function
- This is where we'll intercept player death

**Camera System**:
- `EngineRenderContext` has separate `camera_offset`/`head_offset` fields
- Perfect foundation for adding camera override capability

**Resurrection Station Entities**:
- Resurrection Station template: `-1677` ("Resurrection Station", model: "resust")
- Button template: `-4325` ("Res_Station_Button", model: "res_pad")
- Button has `ResurrectMachine` and `Tweqable` scripts (not yet implemented)
- Both have unparsed `P$DonorType` property (likely activation state)

### Phase 1: Engine Camera Override System

**Files to modify:**
- `engine/src/engine.rs` - Add `CameraOverride` to `EngineRenderContext`
- VR runtime files - Update camera calculation logic

**Implementation:**
1. Add `CameraOverride` struct to `engine/src/engine.rs`:
```rust
pub struct CameraOverride {
    pub transition: f32,    // 0.0 = VR position, 1.0 = override position
    pub position: Vector3<f32>,
    pub forward: Vector3<f32>,
    pub up: Vector3<f32>,
}
```

2. Add `camera_override: Option<CameraOverride>` to `EngineRenderContext`

3. Update VR runtimes to interpolate between VR head position and override when present

### Phase 2: Player Death Detection

**Files to modify:**
- `shock2vr/src/mission/mission_core.rs` - Modify `slay_entity` function
- `shock2vr/src/mission/player_info.rs` - Track player death state

**Implementation:**
1. Detect when player entity is slayed in `mission_core.rs:751`
2. Check if slayed entity is the player (compare with `PlayerInfo.entity_id`)
3. Trigger death sequence instead of normal entity removal

### Phase 3: Death Animation System

**Files to create/modify:**
- `shock2vr/src/systems/player_death.rs` - New system for death animation
- `shock2vr/src/game_scene.rs` - Update render to use camera override

**Implementation:**
1. Create `PlayerDeathState` component:
```rust
pub struct PlayerDeathState {
    pub start_time: f32,
    pub start_position: Vector3<f32>,
    pub target_position: Vector3<f32>, // Floor position
    pub duration: f32,
}
```

2. Death animation lerps camera from current VR position to floor over 2-3 seconds
3. Calculate floor position using physics raycast downward
4. Add random facing direction (forward/backward)

### Phase 4: Resurrection System

**Files to create/modify:**
- `shock2vr/src/scripts/resurrect_machine.rs` - Implement ResurrectMachine script
- `shock2vr/src/mission/nanites.rs` - Nanite currency management

**Implementation:**
1. Create ResurrectMachine script that handles:
   - Button frob detection
   - Player death state check
   - Nanite balance verification (5 nanites required)
   - Teleportation to resurrection station location

2. Add nanite management system:
   - Track player nanite count
   - Deduct 5 nanites on resurrection
   - Store in player properties or separate component

### Phase 5: Integration and Polish

**Files to modify:**
- Mission scene render logic
- Input handling during death state
- UI feedback for resurrection availability

**Implementation:**
1. Disable player input during death animation
2. Show resurrection prompt if available
3. Handle respawn at resurrection station position
4. Reset player health to full on resurrection
5. Clear death state and restore normal camera control

### Recommended Implementation Order

1. **Start with Phase 1** - Camera override system (foundational)
2. **Phase 2** - Player death detection (core logic)
3. **Phase 3** - Death animation (visual feedback)
4. **Phase 4** - Resurrection system (gameplay mechanics)
5. **Phase 5** - Polish and integration

Each phase builds on the previous and can be tested independently. The camera override system from Phase 1 will be reusable for other features like cutscenes or forced camera movements.
