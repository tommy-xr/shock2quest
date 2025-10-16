# VR Teleport Movement System

## Overview
A standard VR teleport system that eliminates motion sickness by allowing players to point to a location and instantly teleport there, rather than smooth locomotion. This system will integrate with the existing shock2quest VR framework.

## Architecture Analysis

### Existing Systems
- **Input System**: `shock2vr/src/input_context.rs` - Handles VR controller input with `Hand` struct containing position, rotation, thumbstick, trigger, squeeze, and A button values
- **Effect System**: `shock2vr/src/scripts/effect.rs` - Provides `SetPlayerPosition` effect with teleport support
- **GUI System**: `shock2vr/src/gui/` - World-space UI rendering with `SetUI` effect for displaying components
- **Existing Teleport**: Basic teleport already exists via `trap_teleport_player.rs` script

## Implementation Plan

### Phase 1: Core Teleport Input Detection ✅ COMPLETED

**Files Created/Modified:**
- ✅ `shock2vr/src/teleport/mod.rs` - Main teleport module
- ✅ `shock2vr/src/teleport/teleport_system.rs` - Core teleport logic
- ✅ `shock2vr/src/lib.rs` - Exported teleport module publicly

**Implementation Completed:**
1. ✅ TeleportSystem struct with configurable input detection
2. ✅ Support for trigger, A button, or squeeze button activation
3. ✅ Per-hand state tracking with button press/release detection
4. ✅ Basic forward ray casting for teleport target (placeholder for Phase 2)
5. ✅ Integration with existing `SetPlayerPosition` effect system
6. ✅ Compiles successfully with desktop runtime

### Phase 2: Arc Trajectory Calculation
**Files to Create/Modify:**
- `shock2vr/src/teleport/trajectory.rs` - Arc calculation and physics

**Implementation:**
1. Calculate parabolic arc from controller position/rotation
2. Perform collision detection with world geometry
3. Determine valid landing position
4. Handle invalid locations (walls, objects, etc.)

### Phase 3: Visual Feedback System
**Files to Create/Modify:**
- `shock2vr/src/teleport/teleport_ui.rs` - Teleport visual components
- `shock2vr/src/teleport/arc_renderer.rs` - Arc line rendering

**Implementation:**
1. Create arc line mesh using existing `lines_mesh.rs` system
2. Landing target indicator (circle/pad at destination)
3. Color coding (green=valid, red=invalid)
4. Integration with existing GUI system via `SetUI` effect

### Phase 4: Teleport Execution
**Files to Create/Modify:**
- `shock2vr/src/teleport/teleport_executor.rs` - Handle actual teleportation

**Implementation:**
1. Trigger on button release
2. Validate final position
3. Use existing `SetPlayerPosition` effect with `is_teleport: true`
4. Clear visual feedback
5. Optional fade transition

### Phase 5: Integration & Configuration
**Files to Create/Modify:**
- `shock2vr/src/teleport/config.rs` - Teleport settings
- Modify `shock2vr/src/lib.rs` to include teleport module

**Implementation:**
1. Configurable arc physics (gravity, max distance)
2. Button mapping options
3. Visual style configuration
4. Performance optimization

## Technical Details

### Input Mapping
- **Primary**: Trigger hold to activate, release to teleport
- **Alternative**: A button hold (configurable)
- **Hand Selection**: Both hands supported, dominant hand priority

### Arc Physics
- **Gravity**: Realistic arc trajectory simulation
- **Max Distance**: Configurable maximum teleport range
- **Collision**: Use existing physics world for ground detection
- **Validation**: Ensure landing spot has adequate clearance

### Visual Components
- **Arc Line**: Segmented line following trajectory path
- **Landing Pad**: Circular indicator at destination
- **Invalid Feedback**: Red coloring when location unsuitable
- **Fade Effect**: Optional screen fade during teleport

### Performance Considerations
- **Update Frequency**: Only calculate when teleport active
- **LOD**: Reduce arc segments at distance
- **Pooling**: Reuse visual meshes to avoid allocation

## Integration Points

### Existing Systems Used
1. **InputContext** - Controller input detection
2. **Effect System** - `SetPlayerPosition` for actual teleportation
3. **GUI System** - World-space UI for visual feedback
4. **Physics** - Collision detection for valid surfaces
5. **Render System** - Line and mesh rendering

### New Components Required
- `TeleportState` - Track active teleport per hand
- `TeleportConfig` - System configuration
- `ArcTrajectory` - Physics calculation
- `TeleportVisuals` - UI components and rendering

## Testing Strategy
1. **Unit Tests**: Arc calculation accuracy
2. **Integration Tests**: Input -> visual -> teleport flow
3. **VR Testing**: Motion sickness validation
4. **Performance Tests**: Frame rate impact measurement

## Configuration Options
```rust
pub struct TeleportConfig {
    pub enabled: bool,
    pub max_distance: f32,
    pub arc_gravity: f32,
    pub button_mapping: TeleportButton,
    pub visual_style: TeleportVisualStyle,
    pub fade_duration: f32,
}

pub enum TeleportButton {
    Trigger,
    AButton,
    Squeeze,
}
```

## Success Criteria
- [ ] Smooth button hold detection without false triggers
- [ ] Accurate arc trajectory that feels natural
- [ ] Clear visual feedback for valid/invalid destinations
- [ ] Instant teleportation with no motion sickness
- [ ] Stable performance with no frame drops
- [ ] Configurable settings for user preference