# Improved Logging Strategy for Shock2Quest

## Current State Analysis

The project currently has inconsistent logging approaches:
- **Performance profiling macros** (`profile!` in `engine/src/macros.rs`) that always output timing
- **Direct println! statements** scattered throughout for debugging
- **Tracing library** included in dependencies but inconsistently used
- **Hard-coded debug prints** like `!!debug - audio update`

### Problems
1. No centralized log level control - all performance logs always print
2. Inconsistent logging methods across codebase
3. Very noisy output - performance timing on every frame
4. No scoping/filtering capabilities
5. Mixed usage of println!, tracing, and debug prints

## Proposed Solution

### 1. Centralized Logging Configuration Module

Create `engine/src/logging/mod.rs` with:
- Configurable log levels and scopes
- Game-specific environment variable support (configurable by each runtime)
- Runtime log level changes via in-game commands

**Runtime Integration:**
- `shock2vr` and `desktop_runtime` call `engine::logging::init_logging("SHOCK2_LOG")`
- Future games can use their own environment variables (e.g., `"THIEF_LOG"`)
- Engine remains game-agnostic while providing flexible logging infrastructure

### 2. Replace Current Logging Infrastructure

- Replace `profile!` macro with scope-aware, level-controlled version
- Convert all `println!` debug statements to proper tracing calls
- Remove hard-coded debug prints
- Standardize on tracing library across all crates

### 3. Define Logging Scopes and Levels

**Scopes:**
- `physics` - Physics simulation, collision detection, movement
- `audio` - Audio playback, spatial audio, music cues
- `render` - Rendering pipeline, scene objects, visibility
- `game` - Core game loop, state management
- `mission` - Mission loading, entity management
- `script` - Script execution, AI behaviors
- `assets` - Asset loading, caching
- `input` - Input handling, VR controllers

**Levels:**
- `error` - Critical errors that may crash or break functionality
- `warn` - Warning conditions that should be addressed
- `info` - General information about major operations
- `debug` - Detailed debugging information
- `trace` - Very verbose tracing, including performance metrics

**Special handling:**
- Performance scope: High-frequency timing logs only at `trace` level
- Optional sampling for high-frequency logs (every Nth frame)

### 4. Enhanced Macro Infrastructure

**New `profile!` macro features:**
- Respects log levels and scopes
- Structured logging with consistent format
- Optional sampling for performance-critical paths
- Example: `profile!(scope: "physics", level: debug, "collision_detection", { ... })`

### 5. Configuration System

**Environment Variables:**
- Configurable environment variable name (e.g., `SHOCK2_LOG`, `THIEF_LOG`)
- Format: `<APP>_LOG=level` - Global log level
- Format: `<APP>_LOG=scope1=level1,scope2=level2` - Per-scope levels
- Examples:
  - `SHOCK2_LOG=warn` - Only warnings and errors
  - `SHOCK2_LOG=info,physics=debug,render=trace` - Mixed levels per scope

**Configuration Files:**
- Game settings file integration
- Runtime configuration changes
- Persistent settings across sessions

### 6. Migration Strategy

**Phase 1: Core Infrastructure ✅ COMPLETED**
- ✅ Implement logging configuration module (`engine/src/logging/`)
- ✅ Update profile! macro with scope-aware functionality
- ✅ Add scope-aware logging utilities and convenience macros

**Phase 2: High-Impact Areas**
- Migrate performance-critical rendering code
- Update physics logging
- Replace audio debug prints

**Phase 3: Systematic Migration**
- Update remaining subsystems one by one
- Convert all println! statements
- Add proper error handling with logging

**Phase 4: Polish**
- Add documentation and examples
- Optimize performance overhead
- Add runtime configuration UI

## Implementation Details

### Logging Configuration Module

```rust
// engine/src/logging/mod.rs
pub struct LogConfig {
    global_level: Level,
    scope_levels: HashMap<String, Level>,
}

impl LogConfig {
    pub fn from_env(env_var_name: &str) -> Self {
        // Parse the specified environment variable (e.g., "SHOCK2_LOG")
    }

    pub fn should_log(&self, scope: &str, level: Level) -> bool {
        // Check if logging should occur for scope/level
    }
}

// Convenience function for initializing with custom env var
pub fn init_logging(env_var_name: &str) -> LogConfig {
    LogConfig::from_env(env_var_name)
}
```

### Enhanced Profile Macro

```rust
#[macro_export]
macro_rules! profile {
    (scope: $scope:expr, level: $level:expr, $description:expr, $block:expr) => {{
        if LOG_CONFIG.should_log($scope, $level) {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            tracing::event!($level, scope = $scope, duration = ?duration, "{}", $description);
            result
        } else {
            $block
        }
    }};
}
```

## Benefits

1. **Reduced Log Noise** - Performance logs only when needed
2. **Targeted Debugging** - Enable specific subsystems for investigation
3. **Consistent Format** - Structured logging across all components
4. **Performance Control** - Disable expensive logging in production
5. **Developer Productivity** - Faster debugging with relevant information
6. **Configurable** - Easy to adjust without code changes

## Example Usage

```bash
# Only show warnings and errors
SHOCK2_LOG=warn cargo run

# Debug physics, trace rendering, warn everything else
SHOCK2_LOG=warn,physics=debug,render=trace cargo run

# Trace everything (very verbose)
SHOCK2_LOG=trace cargo run

# Future game support
THIEF_LOG=debug cargo run --bin thief
```

This strategy provides a comprehensive solution to the current logging noise while maintaining debugging capabilities and adding powerful filtering options.

## Implementation Status

### Phase 1: Core Infrastructure - COMPLETED ✅

**Implemented Files:**
- `engine/src/logging/mod.rs` - Main logging module with static configuration management
- `engine/src/logging/config.rs` - Configuration parsing and log level management
- `engine/src/logging/macros.rs` - Scoped logging convenience macros
- `engine/src/macros.rs` - Enhanced profile! macro with backwards compatibility
- `engine/Cargo.toml` - Added tracing-subscriber dependency
- `engine/src/lib.rs` - Exported logging module

**Features Delivered:**
1. **LogConfig System**: Configurable logging with global and per-scope levels
2. **Environment Variable Support**: Parse logging configuration from environment variables like `SHOCK2_LOG=warn,physics=debug`
3. **Enhanced profile! Macro**:
   - New: `profile!(scope: "physics", level: debug, "description", { code })`
   - Backwards compatible: `profile!("description", { code })` (uses "performance" scope, DEBUG level)
4. **Convenience Macros**: `physics_log!`, `audio_log!`, `render_log!`, etc. for common scopes
5. **Thread-Safe Configuration**: Uses `OnceLock` for global config with lazy defaults

**Usage Examples:**
```rust
// Initialize in runtime (e.g., main.rs)
engine::logging::init_logging("SHOCK2_LOG");

// Enhanced profile macro
profile!(scope: "physics", level: debug, "collision_detection", {
    // expensive physics computation
});

// Backwards compatible
profile!("render_frame", {
    // rendering code - uses "performance" scope, DEBUG level
});

// Scoped logging
physics_log!(warn, "Physics simulation unstable: dt={}", dt);
audio_log!(info, "Loading audio file: {}", filename);
```

**Environment Variable Examples:**
```bash
# Only warnings and errors
SHOCK2_LOG=warn cargo run

# Debug physics, trace rendering, warn everything else
SHOCK2_LOG=warn,physics=debug,render=trace cargo run

# Trace everything (very verbose)
SHOCK2_LOG=trace cargo run
```

**Ready for Phase 2**: The core infrastructure is now in place to begin migrating high-impact areas like physics and rendering to use the new scoped logging system.

## Files to Modify

### New Files to Create
- `engine/src/logging/mod.rs` - Core logging configuration module
- `engine/src/logging/config.rs` - Configuration parsing and management
- `engine/src/logging/macros.rs` - Enhanced logging macros

### Engine Module Updates
- `engine/src/lib.rs` - Export logging module
- `engine/src/macros.rs` - Replace current `profile!` macro with enhanced version
- `engine/Cargo.toml` - Add any additional logging dependencies if needed

### Runtime Integration
- `runtimes/desktop_runtime/src/main.rs` - Initialize logging with "SHOCK2_LOG"
- `runtimes/oculus_runtime/src/lib.rs` - Initialize logging with "SHOCK2_LOG"
- `runtimes/tool/src/main.rs` - Initialize logging (if applicable)

### Core Game Libraries
- `shock2vr/src/lib.rs` - Replace println! and update tracing usage
- `shock2vr/src/physics/mod.rs` - Convert performance logging to scoped system
- `shock2vr/src/mission/mod.rs` - Update logging calls
- `dark/src/gamesys/gamesys.rs` - Update logging statements
- `dark/src/motion/mod.rs` - Convert debug prints to proper logging
- `dark/src/model.rs` - Update any logging statements
- `dark/src/properties/mod.rs` - Update logging if present

### Audio System
- `engine/src/audio/mod.rs` - Remove `!!debug - audio update` and similar prints
- `dark/src/audio/` - Update any audio-related logging

### Subsystem Files (Replace println! with scoped logging)
- `shock2vr/src/mission/entity_creator.rs`
- `shock2vr/src/virtual_hand.rs`
- `dark/src/gamesys/env_map.rs`
- `dark/src/gamesys/sound_schema.rs`
- `dark/src/gamesys/speech_db.rs`
- `dark/src/importers/audio_importer.rs`
- `dark/src/importers/strings_importer.rs`
- `dark/src/motion/animation_player.rs`
- `dark/src/motion/motion_clip.rs`
- `dark/src/properties/prop_ai.rs`
- `dark/src/properties/prop_tweq.rs`
- `dark/src/ss2_bin_ai_loader.rs`
- `dark/src/ss2_bin_obj_loader.rs`
- `dark/src/ss2_cal_loader.rs`
- `dark/src/ss2_skeleton.rs`
- `shock2vr/src/systems/tweq.rs`

### Scripts and AI System
- Files in `shock2vr/src/scripts/` that contain logging
- `shock2vr/src/scripts/ai/animated_monster_ai.rs`

### Scene and Engine Files
- `engine/src/scene/scene_object.rs`
- `engine/src/importers/fbx_importer.rs`
- `engine/src/assets/asset_paths.rs`

### Priority Order for Migration
1. **Core infrastructure** (`engine/src/logging/`, `engine/src/macros.rs`)
2. **Runtime initialization** (all `runtimes/*/src/main.rs` and `lib.rs`)
3. **High-frequency logging** (`shock2vr/src/physics/mod.rs`, `shock2vr/src/lib.rs`)
4. **Audio system** (`engine/src/audio/mod.rs` and related)
5. **Subsystem files** (all the remaining files with println! statements)

### Files with Current Logging Issues
Based on git status, these files already have modifications and may need logging updates:
- `dark/src/gamesys/env_map.rs`
- `dark/src/gamesys/gamesys.rs`
- `dark/src/gamesys/sound_schema.rs`
- `dark/src/gamesys/speech_db.rs`
- `dark/src/importers/audio_importer.rs`
- `dark/src/importers/strings_importer.rs`
- `dark/src/model.rs`
- `dark/src/motion/animation_player.rs`
- `dark/src/motion/mod.rs`
- `dark/src/motion/motion_clip.rs`
- `dark/src/properties/mod.rs`
- `dark/src/properties/prop_ai.rs`
- `dark/src/properties/prop_tweq.rs`
- `dark/src/ss2_bin_ai_loader.rs`
- `dark/src/ss2_bin_obj_loader.rs`
- `dark/src/ss2_cal_loader.rs`
- `dark/src/ss2_skeleton.rs`
- `engine/src/macros.rs`
- `shock2vr/src/systems/tweq.rs`