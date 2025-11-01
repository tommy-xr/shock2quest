# Mission Core Refactoring Plan

## Overview

This document outlines the plan to refactor the `Mission` struct to generalize its functionality, enabling debug scenes to access advanced game systems like physics, entities, animation, teleport, and scripting without requiring System Shock 2 level data.

## Current Problem

The current `Mission` struct (shock2vr/src/mission/mod.rs, ~2000 lines) tightly couples:
- **SS2-specific level loading** (`dark::mission::read`, `SystemShock2Level`, `SystemShock2EntityInfo`)
- **Generic game scene functionality** (physics, rendering, entity management, scripts, GUI, etc.)

This prevents debug scenes from accessing advanced systems, limiting them to basic rendering and simple physics.

## Goal

Enable debug scenes to simulate complex behaviors like:
- Teleportation systems
- Ragdoll physics
- Advanced entity interactions
- Script-based behaviors
- GUI systems
- Animation systems
- Complex physics scenarios

## Architecture Design

### 1. Core Abstraction: MissionCore

Create a new `MissionCore` struct that contains all generic game scene functionality:

```rust
pub struct MissionCore {
    // ECS and Core Systems
    pub world: World,
    pub physics: PhysicsWorld,
    pub script_world: ScriptWorld,

    // Rendering Systems
    pub scene_objects: Vec<SceneObject>,
    pub id_to_model: HashMap<EntityId, Model>,
    pub id_to_animation_player: HashMap<EntityId, AnimationPlayer>,
    pub id_to_bitmap: HashMap<EntityId, Rc<BitmapAnimation>>,
    pub id_to_particle_system: HashMap<EntityId, ParticleSystem>,
    pub id_to_physics: HashMap<EntityId, RigidBodyHandle>,

    // Player Systems
    pub player_handle: PlayerHandle,
    pub left_hand: VirtualHand,
    pub right_hand: VirtualHand,

    // Game Systems
    pub gui: GuiManager,
    pub hit_boxes: HitBoxManager,
    pub teleport_system: TeleportSystem,
    pub visibility_engine: Box<dyn VisibilityEngine>,

    // Debug and Utility
    pub debug_lines: Vec<DebugLine>,
    pub pending_entity_triggers: Vec<String>,

    // Metadata
    pub scene_name: String,
    pub template_name_to_template_id: HashMap<String, EntityMetadata>,
}
```

### 2. MissionCore API

`MissionCore` implements all the core `GameScene` functionality:

```rust
impl MissionCore {
    // Constructor with configurable options
    pub fn new(scene_name: String, game_options: &GameOptions) -> Self;

    // Entity management (moved from Mission)
    pub fn create_entity_with_position(...) -> EntityCreationInfo;
    pub fn remove_entity(&mut self, entity_id: EntityId);
    pub fn slay_entity(&mut self, entity_id: EntityId, asset_cache: &mut AssetCache) -> bool;

    // Physics management
    pub fn make_physical(&mut self, entity_id: EntityId);
    pub fn make_un_physical(&mut self, entity_id: EntityId);
    pub fn set_entity_position_rotation(...);

    // All current Mission methods that don't involve SS2-specific data
}

impl GameScene for MissionCore {
    // Full GameScene implementation moved from Mission
}
```

### 3. Refactored Mission

`Mission` becomes a wrapper around `MissionCore`:

```rust
pub struct Mission {
    // SS2-specific data
    pub level: SystemShock2Level,
    pub entity_info: SystemShock2EntityInfo,

    // Generic functionality delegated to core
    pub core: MissionCore,
}

impl Mission {
    pub fn load(...) -> Mission {
        // 1. Load SS2 level data (existing logic)
        let level = dark::mission::read(...);
        let entity_info = ss2_entity_info::merge_with_gamesys(...);

        // 2. Create MissionCore
        let mut core = MissionCore::new(mission_name, game_options);

        // 3. Populate core with SS2 data
        populate_core_from_level(&mut core, &level, &entity_info, ...);

        Mission { level, entity_info, core }
    }
}

impl GameScene for Mission {
    // Delegate all methods to core
    fn update(&mut self, ...) -> Vec<Effect> {
        self.core.update(...)
    }

    fn render(&mut self, ...) -> (...) {
        self.core.render(...)
    }

    // ... all other methods delegate to core
}
```

### 4. Enhanced Debug Scenes

Debug scenes can now create their own `MissionCore` and populate it with test data:

```rust
pub struct DebugTeleportScene {
    core: MissionCore,
}

impl DebugTeleportScene {
    pub fn new() -> Self {
        let mut core = MissionCore::new("debug_teleport".to_string(), &default_options());

        // Set up teleport system
        core.teleport_system = TeleportSystem::new(TeleportConfig {
            enabled: true,
            max_distance: 20.0,
            // ... configure for testing
        });

        // Add test entities for teleport targets
        Self::create_test_environment(&mut core);

        Self { core }
    }

    fn create_test_environment(core: &mut MissionCore) {
        // Create platforms, obstacles, interactive objects
        // Add entities with proper physics and scripts
        // Set up complex test scenarios
    }
}

impl GameScene for DebugTeleportScene {
    // Delegate everything to core
    fn update(&mut self, ...) -> Vec<Effect> {
        self.core.update(...)
    }
    // ... etc
}
```

## Implementation Phases

### Phase 1: Extract MissionCore Structure
1. Create `shock2vr/src/mission/mission_core.rs`
2. Move all non-SS2-specific fields from `Mission` to `MissionCore`
3. Update imports and references

### Phase 2: Extract MissionCore Methods
1. Move all non-SS2-specific methods from `Mission` to `MissionCore`
2. Update method signatures to work with `MissionCore`
3. Ensure all functionality is preserved

### Phase 3: Implement GameScene for MissionCore
1. Move `GameScene` implementation from `Mission` to `MissionCore`
2. Handle SS2-specific methods (like `ambient_audio_state`) appropriately

### Phase 4: Refactor Mission as Wrapper
1. Refactor `Mission` to contain `MissionCore`
2. Implement `GameScene` for `Mission` as delegation to `core`
3. Update `Mission::load` to populate `MissionCore`

### Phase 5: Create Population Helper
1. Extract SS2 data population logic into `populate_core_from_level`
2. This function takes SS2 data and populates a `MissionCore` instance
3. Used by both `Mission::load` and potentially debug scenes that want SS2 data

### Phase 6: Create DebugEntityPlayground Prototype
1. Create `DebugEntityPlaygroundScene` to validate the refactoring
2. Spawn real SS2 entities from shock2.gam on a simple plane
3. Test VR interaction, scripts, physics with real game entities
4. Update existing debug scenes to use `MissionCore`

### DebugEntityPlayground Prototype Details

This prototype scene will be the key validation of the refactoring:

**Setup**: Simple floor plane with real SS2 entities spawned from templates
**Entities**: Pistol, Maintenance Tool, Security Crate, Med Bed, Turret, Cyber Module, Door
**Features**: VR hand interaction, grabbing/throwing, teleport movement
**Benefits**: Validates entity creation, scripts, physics, asset loading with real game data

```rust
pub struct DebugEntityPlaygroundScene {
    core: MissionCore,
}

impl DebugEntityPlaygroundScene {
    pub fn new(global_context: &GlobalContext) -> Self {
        let mut core = MissionCore::new("debug_playground".to_string(), &default_options());

        // Create template mapping from gamesys
        core.template_name_to_template_id = create_template_name_map(&global_context.gamesys);

        // Set up simple environment
        Self::create_test_environment(&mut core);

        // Spawn initial entities
        Self::spawn_test_entities(&mut core);

        Self { core }
    }
}
```

This prototype perfectly exercises the boundary between SS2-specific data and generic scene functionality.

## Benefits

### For Debug Scenes
- **Full System Access**: Debug scenes can use teleport, GUI, animation, scripting, complex physics
- **Realistic Testing**: Test features in isolation with proper game system context
- **Rapid Prototyping**: Quickly create test scenarios without SS2 level complexity

### For Mission System
- **Clean Separation**: SS2-specific logic clearly separated from generic game logic
- **Better Testability**: Core game systems can be tested independently
- **Maintainability**: Easier to modify core systems without touching SS2-specific code

### For Future Development
- **Extensibility**: Easy to add new scene types (cutscenes, UI screens, etc.)
- **Reusability**: Core systems can be reused for different game modes
- **Modularity**: Clear boundaries between different system responsibilities

## Example Usage

### Debug Ragdoll Scene
```rust
let mut core = MissionCore::new("debug_ragdoll".to_string(), options);

// Create a character with ragdoll physics
let character_entity = core.create_entity_by_template_name(
    asset_cache, "Human", position, orientation
);

// Set up ragdoll physics configuration
core.physics.configure_ragdoll(character_entity, ...);

// Add trigger to activate ragdoll on input
// Test different ragdoll parameters
```

### Debug Teleport Scene
```rust
let mut core = MissionCore::new("debug_teleport".to_string(), options);

// Enable teleport system with test configuration
core.teleport_system = TeleportSystem::new(experimental_config);

// Create test environment with platforms, obstacles
Self::create_teleport_test_level(&mut core);

// Test teleport mechanics in controlled environment
```

## Risk Mitigation

### Backwards Compatibility
- All existing `Mission` functionality preserved
- `GameScene` interface unchanged
- No changes to calling code initially required

### Incremental Migration
- Changes can be made incrementally
- Each phase can be tested independently
- SS2 missions continue working throughout refactoring

### Testing Strategy
- Existing SS2 missions must continue working
- Debug scenes should gain new capabilities
- All `GameScene` methods must work correctly
- Physics, rendering, and entity systems must be unaffected

## File Structure

```
shock2vr/src/mission/
├── mod.rs                    # Updated Mission struct
├── mission_core.rs           # New MissionCore implementation
├── level_population.rs       # SS2 -> MissionCore population logic
└── ... (existing files)

shock2vr/src/scenes/
├── mod.rs                    # Updated scene selection
├── debug_minimal.rs          # Refactored to use MissionCore
├── debug_physics.rs          # Refactored to use MissionCore
├── debug_teleport.rs         # New advanced debug scene
├── debug_ragdoll.rs          # New advanced debug scene
└── debug_hud.rs              # New HUD testing scene
```

This refactoring enables the creation of sophisticated debug scenes that can test complex game features like teleportation and ragdoll physics while maintaining the existing SS2 mission functionality.