# Climbing Implementation Plan

## Overview

Implement VR hand-grip climbing for ladders, with desktop adaptation. Focus on ladder climbing first (climbable_sides bitmask), mantling as follow-up.

## Goals

- Implement climbing ladders in the VR runtime (hand-grip based)
- Implement climbing ladders in the desktop runtime (Half-Life style)
- Implement automatic mantling for both platforms
- Jumping is a separate task (not needed for climbing)

## Key Technical Insights

### Climbable Sides Bitmask (from Dark Engine)

The `PropPhysAttr.climbable` field is a 6-bit bitmask for OBB faces:
- `1` = +X, `2` = +Y, `4` = +Z (top), `8` = -X, `16` = -Y, `32` = -Z (bottom)
- **Value 27 = 1+2+8+16** = four vertical sides (+X, +Y, -X, -Y) - typical ladder

See `references/dark_engine_climbing_info.md` for details.

### Detection Logic (Original Engine)

1. Player touching OBB face whose bit is set
2. Player facing into that face's normal
3. Switch to climb mode, constrain movement
4. Suppress normal collisions to allow attachment

### VR Climbing Loop

1. Hand overlaps climbable + squeeze threshold + empty hand -> valid grip
2. On grip: store `handAnchor = handPoseWorld`
3. While gripping: `delta = currentHandPose - handAnchor`, move player by `-delta`
4. On release: apply velocity from controller
5. Gravity: zero while any hand gripping, restore when released

---

## Phase 1: Climbable Surface Detection Infrastructure

**Goal:** Detect when player/hands are touching climbable surfaces

### Tasks

1. **Add `CLIMBABLE` collision group** in `shock2vr/src/physics/mod.rs`
   - New bit in `InternalCollisionGroups`
   - Entities with `PropPhysAttr.climbable != 0` get this group

2. **Mark climbable entities during creation** in `shock2vr/src/mission/entity_creator.rs`
   - When creating physics bodies, check `PropPhysAttr.climbable`
   - If non-zero, add to CLIMBABLE collision group

3. **Add hand collision sensors** (VR)
   - Create small sphere sensors at hand positions
   - Query for CLIMBABLE intersections each frame

4. **Add player-climbable intersection query** (Desktop fallback)
   - Check if player body overlaps any CLIMBABLE collider

### Files to Modify

- `shock2vr/src/physics/mod.rs` - collision groups, hand sensors
- `shock2vr/src/mission/entity_creator.rs` - mark climbable entities

### Testing

- Debug log when player/hand touches climbable surface
- Verify ladders in earth.mis are detected

---

## Phase 2: Movement Mode Abstraction

**Goal:** Extract movement logic into pluggable modes

### Tasks

1. **Create `MovementMode` enum** in new file `shock2vr/src/physics/movement.rs`
   ```rust
   pub enum MovementMode {
       Walk,
       Climb { anchor: Option<ClimbAnchor> },
   }

   pub struct ClimbAnchor {
       pub hand: Hand,  // Left or Right
       pub world_position: Vector3<f32>,
   }
   ```

2. **Add `PlayerMovementState`** to track current mode
   ```rust
   pub struct PlayerMovementState {
       pub mode: MovementMode,
       pub left_hand_anchor: Option<Vector3<f32>>,
       pub right_hand_anchor: Option<Vector3<f32>>,
   }
   ```

3. **Refactor `mission_core.rs` movement handling**
   - Extract current walk logic into helper function
   - Add match on movement mode to select behavior
   - Keep changes minimal - just structural refactor

### Files to Modify

- New: `shock2vr/src/physics/movement.rs`
- `shock2vr/src/physics/mod.rs` - re-export movement types
- `shock2vr/src/mission/mission_core.rs` - refactor movement dispatch

### Testing

- Existing walk movement unchanged
- Game plays identically to before

---

## Phase 3: VR Climb Mode Implementation

**Goal:** Implement hand-grip climbing for VR

### Tasks

1. **Create debug climbing scene**
   - Add simple climbable entity (tall box with `climbable: 27`)
   - Spawn near player start for quick iteration
   - Could be a debug command or test mission setup

2. **Grip detection logic**
   - Check `squeeze_value > CLIMB_GRIP_THRESHOLD` (const, ~0.8)
   - Check hand overlaps CLIMBABLE sensor
   - Check hand is empty via `VirtualHand::get_held_entity().is_none()`
     - Located in `shock2vr/src/virtual_hand.rs`
     - Access via `self.left_hand` / `self.right_hand` in mission_core

3. **Anchor management**
   - On grip start: `anchor = hand.position`
   - On grip release: clear anchor, apply velocity

4. **Climb movement calculation**
   ```rust
   // Each frame while gripping:
   let delta = current_hand_pos - anchor;
   let player_movement = -delta;  // Move player opposite to hand movement
   anchor = current_hand_pos;     // Update anchor
   ```

5. **Gravity control**
   - While any hand anchored: set player gravity to 0
   - When both released: restore gravity

6. **Release velocity**
   - Track hand velocity over last few frames
   - On release, apply as player velocity (scaled)

### Files to Modify

- `shock2vr/src/physics/movement.rs` - climb logic
- `shock2vr/src/mission/mission_core.rs` - integrate climb mode
- `shock2vr/src/physics/mod.rs` - gravity control API

### Testing

- Use debug climbing scene for iteration
- Grip with VR controllers
- Pull down to climb up
- Release and fall with momentum

---

## Phase 4: Desktop Ladder Climbing

**Goal:** Half-Life style ladder climbing for desktop

### Tasks

1. **Ladder attachment detection**
   - Player body touching CLIMBABLE collider
   - Looking toward the climbable face (dot product check)

2. **Desktop climb mode behavior**
   - Forward/back keys -> up/down on ladder
   - Strafe keys -> left/right along ladder
   - Gravity disabled while attached

3. **Detachment conditions**
   - Jump key (when implemented)
   - Moving away from ladder
   - Reaching top/bottom

### Files to Modify

- `shock2vr/src/physics/movement.rs` - desktop climb variant
- `shock2vr/src/mission/mission_core.rs` - desktop input mapping

### Testing

- Approach ladder on desktop
- W/S to climb up/down
- Move away to detach

---

## Phase 5: Polish & Edge Cases

**Goal:** Handle edge cases and improve feel

### Tasks

1. **Smooth transitions** between walk and climb modes
2. **Prevent clipping** through ladder geometry
3. **Audio feedback** for grip/release (if sound system supports)
4. **Top-of-ladder handling** - smooth transition to standing on top
5. **Haptic feedback** - vibrate controller on grip/release
6. **Hand gesture on climbable** - change hand pose when near climbable surfaces (open hand -> grab pose)

### Files to Modify

- Various based on specific issues found
- VR runtime for haptics integration

---

## Phase 6: Automatic Mantling

**Goal:** Automatically pull player up onto ledges when near climbable surfaces

Note: Mantling in Dark Engine uses texture-level "Climbability" float, not `climbable_sides`. However, for VR we can simplify with geometry-based detection.

### Tasks

1. **Ledge detection**
   - Cast ray forward from player chest height
   - If blocked, cast second ray from higher point
   - If second ray clear, there's a ledge to mantle

2. **Mantle trigger conditions**
   - Player near climbable surface (or any solid surface)
   - Player moving toward surface
   - Ledge detected within mantle reach (~1m above player)

3. **VR mantle behavior**
   - When gripping near top of climbable surface + pulling up
   - Smoothly translate player up and over the ledge
   - Transition back to walk mode on flat surface

4. **Desktop mantle behavior**
   - Automatic when moving into ledge at appropriate height
   - Smooth camera transition to avoid jarring movement

### Files to Modify

- `shock2vr/src/physics/movement.rs` - mantle logic
- `shock2vr/src/physics/mod.rs` - ledge detection raycasts

### Testing

- Find waist-high obstacles
- Walk/climb toward them
- Verify smooth transition onto ledge

---

## Critical Files Summary

| File | Purpose |
|------|---------|
| `shock2vr/src/physics/mod.rs` | Collision groups, gravity control, hand sensors |
| `shock2vr/src/physics/movement.rs` | NEW - movement mode logic |
| `shock2vr/src/mission/mission_core.rs` | Movement dispatch, input processing |
| `shock2vr/src/mission/entity_creator.rs` | Mark climbable entities |
| `shock2vr/src/virtual_hand.rs` | Hand state (empty check via `get_held_entity()`) |
| `dark/src/properties/prop_phys_attr.rs` | PropPhysAttr (already has climbable field) |

## Design Decisions

- **VR requires empty hand** - must holster items before grabbing ladder
- **VR-first implementation** - desktop adapts from VR approach
- **Climbable value 27 only** initially (four vertical sides)
- **Automatic mantling** - triggers when near ledge and moving toward it
- **Squeeze threshold as constant** - `CLIMB_GRIP_THRESHOLD` for easy tuning

## Out of Scope (Separate Tasks)

- **Jumping** - separate prerequisite task (not needed for climbing)
- **Vines/ropes** - may need different physics model (rope simulation)
