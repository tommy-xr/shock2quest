# Melee Combat System

## Project Status: 📋 PLANNING

This document describes the implementation plan for a proper melee combat system that works well in both VR and desktop environments.

## Overview

System Shock 2 features several melee weapons (Wrench, Laser Rapier, Crystal Shard) that require a physics-informed combat system. The current implementation (`melee_weapon.rs`) is minimal—it just sends `Damage { amount: 1.0 }` on any collision.

### Design Goals

1. **VR-first experience**: Swinging weapons should feel natural and rewarding
2. **Desktop parity**: Desktop players get a functional melee experience via pre-programmed swing arcs
3. **Velocity-based damage**: Faster swings = more damage (rewards good VR technique)
4. **Material-aware feedback**: Hit sounds based on Dark Engine material system
5. **Extensible foundation**: Architecture supports future blocking, stamina, etc.

## Current State Analysis

### Existing Infrastructure

| Component | Location | Status |
|-----------|----------|--------|
| Melee weapon script | `shock2vr/src/scripts/melee_weapon.rs` | Minimal (collision → damage) |
| VR hand tracking | `shock2vr/src/virtual_hand.rs` | Full position/rotation tracking |
| Hitbox system | `shock2vr/src/creature/hit_boxes.rs` | Per-joint hitboxes with multipliers |
| Physics raycasting | `shock2vr/src/physics/mod.rs` | `ray_cast`, `ray_cast2`, `ray_cast3` |
| Damage messaging | `shock2vr/src/scripts/mod.rs` | `MessagePayload::Damage { amount }` |
| VR weapon config | `shock2vr/src/vr_config.rs` | Hand model offsets per weapon |
| Material property | `dark/src/properties/mod.rs` | `PropMaterial(String)` parsed |

### Melee Weapons in Game

| Weapon | Template Name | Description |
|--------|---------------|-------------|
| Wrench | `Wrench` | Starting melee weapon, low damage |
| Laser Rapier | `Laser Rapier` | Energy-based sword, higher damage |
| Crystal Shard | `Crystal Shard` | Psi-powered melee weapon |

## Architecture: Hybrid Kinematic + Velocity Tracking

After evaluating three approaches (raycast arc, full physics, hybrid), we chose **Hybrid** for these reasons:

1. **Kinematic positioning** - Weapon follows hand exactly, no jitter/clipping
2. **Velocity tracking** - We track weapon tip/base velocity for damage calculation
3. **Swept collision** - Raycasts from previous to current position catch fast swings
4. **Platform abstraction** - VR gets real velocity, desktop gets synthetic velocity from arc

```
┌─────────────────────────────────────────────────────────────────┐
│                     MELEE COMBAT SYSTEM                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────────┐      ┌─────────────────────────────┐  │
│  │   VR Controller     │      │   Desktop Mouse/Keyboard    │  │
│  │  (physical motion)  │      │  (attack button → swing)    │  │
│  └──────────┬──────────┘      └──────────────┬──────────────┘  │
│             │                                │                  │
│             ▼                                ▼                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  MeleeWeaponState                         │  │
│  │  ┌────────────────┐  ┌─────────────────────────────────┐ │  │
│  │  │ VelocityTracker│  │ DesktopSwingController          │ │  │
│  │  │ - prev_tip_pos │  │ - swing_arc: SwingArc           │ │  │
│  │  │ - prev_base_pos│  │ - swing_t: f32 (0.0 → 1.0)      │ │  │
│  │  │ - tip_velocity │  │ - is_swinging: bool             │ │  │
│  │  │ - base_velocity│  │ - synthetic_velocity: Vec3      │ │  │
│  │  └────────────────┘  └─────────────────────────────────┘ │  │
│  │  - hit_entities_this_swing: HashSet<EntityId>            │  │
│  │  - swing_start_time: Option<Instant>                     │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Swept Collision Detection                    │  │
│  │  - Cast rays along weapon edge (tip, mid, base)          │  │
│  │  - Cast from prev_position → curr_position (motion)      │  │
│  │  - Filter: HITBOX | ENTITY collision groups              │  │
│  │  - Skip entities already hit this swing                  │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  Damage Calculation                       │  │
│  │  damage = base_damage                                     │  │
│  │         × velocity_factor (0.5 - 2.0)                    │  │
│  │         × hitbox_multiplier (head=2x, limb=0.5x, etc)    │  │
│  │         × weapon_modifier (from weapon stats)            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Hit Feedback                           │  │
│  │  - Impact sound (material-based via Dark Engine system)  │  │
│  │  - VR haptics (controller vibration)                     │  │
│  │  - Visual feedback (blood/sparks particle effect)        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Phase 1: Core Velocity Tracking & Hit Detection

**Goal**: Track weapon velocity and detect hits via swept raycasts.

#### 1.1 Create Melee Combat Module

**New file**: `shock2vr/src/melee/mod.rs`

```rust
pub mod velocity_tracker;
pub mod hit_detection;
pub mod damage_calculator;
pub mod weapon_config;
```

#### 1.2 Velocity Tracker

**New file**: `shock2vr/src/melee/velocity_tracker.rs`

```rust
use cgmath::{Vector3, InnerSpace};

pub struct VelocityTracker {
    prev_tip_position: Option<Vector3<f32>>,
    prev_base_position: Option<Vector3<f32>>,
    tip_velocity: Vector3<f32>,
    base_velocity: Vector3<f32>,
    velocity_history: VecDeque<f32>,  // Smoothing
}

impl VelocityTracker {
    pub fn new() -> Self { /* ... */ }

    /// Call each frame with current weapon tip/base world positions
    pub fn update(&mut self, tip: Vector3<f32>, base: Vector3<f32>, dt: f32) {
        if let (Some(prev_tip), Some(prev_base)) = (self.prev_tip_position, self.prev_base_position) {
            self.tip_velocity = (tip - prev_tip) / dt;
            self.base_velocity = (base - prev_base) / dt;

            // Smooth velocity over last few frames
            let magnitude = self.tip_velocity.magnitude();
            self.velocity_history.push_back(magnitude);
            if self.velocity_history.len() > 3 {
                self.velocity_history.pop_front();
            }
        }

        self.prev_tip_position = Some(tip);
        self.prev_base_position = Some(base);
    }

    /// Get smoothed velocity magnitude for damage calculation
    pub fn smoothed_velocity(&self) -> f32 {
        let sum: f32 = self.velocity_history.iter().sum();
        sum / self.velocity_history.len().max(1) as f32
    }

    /// Get previous positions for swept collision
    pub fn previous_positions(&self) -> Option<(Vector3<f32>, Vector3<f32>)> {
        match (self.prev_tip_position, self.prev_base_position) {
            (Some(tip), Some(base)) => Some((tip, base)),
            _ => None,
        }
    }
}
```

#### 1.3 Swept Collision Detection

**New file**: `shock2vr/src/melee/hit_detection.rs`

```rust
use crate::physics::{PhysicsWorld, RayCastResult, CollisionGroups};

pub struct HitDetector;

impl HitDetector {
    /// Perform swept collision detection for melee weapon
    /// Returns first hit found
    pub fn check_melee_hit(
        physics: &PhysicsWorld,
        prev_tip: Vector3<f32>,
        curr_tip: Vector3<f32>,
        prev_base: Vector3<f32>,
        curr_base: Vector3<f32>,
        weapon_entity: EntityId,
    ) -> Option<MeleeHitResult> {
        // Cast along weapon edge at multiple points
        let hit_points = [
            (prev_tip, curr_tip),           // Tip (highest velocity)
            (
                lerp(prev_tip, prev_base, 0.5),
                lerp(curr_tip, curr_base, 0.5)
            ),                               // Middle
        ];

        let collision_groups = CollisionGroups::HITBOX | CollisionGroups::ENTITY;

        for (from, to) in hit_points {
            // Cast from previous to current position (catches fast swings)
            if let Some(hit) = physics.ray_cast3(from, to, collision_groups, Some(weapon_entity), false) {
                return Some(MeleeHitResult {
                    entity_id: hit.maybe_entity_id?,
                    hit_point: hit.hit_point,
                    hit_normal: hit.hit_normal,
                    is_hitbox: hit.is_hitbox,
                });
            }
        }

        None
    }
}

pub struct MeleeHitResult {
    pub entity_id: EntityId,
    pub hit_point: Vector3<f32>,
    pub hit_normal: Vector3<f32>,
    pub is_hitbox: bool,
}
```

#### 1.4 Enhanced MeleeWeapon Script

**Modify**: `shock2vr/src/scripts/melee_weapon.rs`

```rust
use crate::melee::{VelocityTracker, HitDetector, DamageCalculator, WeaponConfig};
use std::collections::HashSet;

pub struct MeleeWeapon {
    velocity_tracker: VelocityTracker,
    hit_entities_this_swing: HashSet<EntityId>,
    weapon_config: WeaponConfig,
    is_swinging: bool,
    swing_start_velocity: f32,
}

impl Script for MeleeWeapon {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        // Get current weapon tip/base positions from entity transform
        let (tip_pos, base_pos) = self.get_weapon_positions(entity_id, world);

        // Update velocity tracking
        self.velocity_tracker.update(tip_pos, base_pos, time.elapsed.as_secs_f32());

        // Detect swing start (velocity crosses threshold)
        let velocity = self.velocity_tracker.smoothed_velocity();
        if !self.is_swinging && velocity > SWING_START_THRESHOLD {
            self.is_swinging = true;
            self.swing_start_velocity = velocity;
            self.hit_entities_this_swing.clear();
        }

        // Detect swing end (velocity drops)
        if self.is_swinging && velocity < SWING_END_THRESHOLD {
            self.is_swinging = false;
        }

        // Only check for hits during active swing
        if !self.is_swinging {
            return Effect::NoEffect;
        }

        // Swept collision detection
        if let Some((prev_tip, prev_base)) = self.velocity_tracker.previous_positions() {
            if let Some(hit) = HitDetector::check_melee_hit(
                physics,
                prev_tip, tip_pos,
                prev_base, base_pos,
                entity_id,
            ) {
                // Skip if already hit this swing
                if self.hit_entities_this_swing.contains(&hit.entity_id) {
                    return Effect::NoEffect;
                }
                self.hit_entities_this_swing.insert(hit.entity_id);

                // Calculate damage
                let damage = DamageCalculator::calculate(
                    &self.weapon_config,
                    velocity,
                    hit.is_hitbox,
                    // TODO: Get hitbox type for multiplier
                );

                // Return combined effects: damage + sound + haptics
                return Effect::combine(vec![
                    Effect::Send {
                        msg: Message {
                            to: hit.entity_id,
                            payload: MessagePayload::Damage { amount: damage },
                        },
                    },
                    // TODO: Impact sound effect
                    // TODO: Haptic feedback effect
                ]);
            }
        }

        Effect::NoEffect
    }
}
```

#### 1.5 Weapon Configuration

**New file**: `shock2vr/src/melee/weapon_config.rs`

```rust
pub struct WeaponConfig {
    pub base_damage: f32,
    pub velocity_scale: f32,      // How much velocity affects damage
    pub min_velocity: f32,        // Minimum velocity to deal damage
    pub weapon_length: f32,       // Distance from base to tip
    pub weapon_material: String,  // For impact sounds
}

impl WeaponConfig {
    pub fn wrench() -> Self {
        WeaponConfig {
            base_damage: 8.0,
            velocity_scale: 1.5,
            min_velocity: 2.0,
            weapon_length: 0.4,
            weapon_material: "Metal".to_string(),
        }
    }

    pub fn laser_rapier() -> Self {
        WeaponConfig {
            base_damage: 15.0,
            velocity_scale: 2.0,
            min_velocity: 1.5,
            weapon_length: 0.8,
            weapon_material: "Energy".to_string(),
        }
    }

    pub fn crystal_shard() -> Self {
        WeaponConfig {
            base_damage: 12.0,
            velocity_scale: 1.0,
            min_velocity: 1.0,
            weapon_length: 0.3,
            weapon_material: "Crystal".to_string(),
        }
    }
}
```

---

### Phase 2: Desktop Swing System

**Goal**: Provide desktop players with a functional melee experience via pre-programmed swing arcs.

#### 2.1 Swing Arc Definition

**New file**: `shock2vr/src/melee/desktop_swing.rs`

```rust
use cgmath::{Vector3, Quaternion};

pub enum SwingType {
    Horizontal,  // Side-to-side
    Overhead,    // Top-down
    Thrust,      // Forward stab
}

pub struct SwingArc {
    swing_type: SwingType,
    duration: f32,
    keyframes: Vec<SwingKeyframe>,
}

pub struct SwingKeyframe {
    t: f32,  // 0.0 - 1.0
    position_offset: Vector3<f32>,
    rotation_offset: Quaternion<f32>,
}

impl SwingArc {
    pub fn horizontal() -> Self {
        SwingArc {
            swing_type: SwingType::Horizontal,
            duration: 0.35,  // 350ms swing
            keyframes: vec![
                SwingKeyframe { t: 0.0, /* wind-up right */ },
                SwingKeyframe { t: 0.3, /* peak velocity mid */ },
                SwingKeyframe { t: 0.6, /* follow-through left */ },
                SwingKeyframe { t: 1.0, /* return to rest */ },
            ],
        }
    }

    /// Get position/rotation at time t, plus synthetic velocity
    pub fn sample(&self, t: f32) -> (Vector3<f32>, Quaternion<f32>, Vector3<f32>) {
        // Interpolate between keyframes
        // Calculate velocity from position delta
        // ...
    }
}
```

#### 2.2 Desktop Swing Controller

**New file**: `shock2vr/src/melee/desktop_swing_controller.rs`

```rust
pub struct DesktopSwingController {
    current_swing: Option<ActiveSwing>,
}

struct ActiveSwing {
    arc: SwingArc,
    start_time: Instant,
}

impl DesktopSwingController {
    pub fn start_swing(&mut self, swing_type: SwingType) {
        if self.current_swing.is_some() {
            return;  // Already swinging
        }
        self.current_swing = Some(ActiveSwing {
            arc: SwingArc::for_type(swing_type),
            start_time: Instant::now(),
        });
    }

    pub fn update(&mut self, dt: f32) -> Option<SwingSample> {
        let swing = self.current_swing.as_ref()?;
        let elapsed = swing.start_time.elapsed().as_secs_f32();
        let t = elapsed / swing.arc.duration;

        if t >= 1.0 {
            self.current_swing = None;
            return None;
        }

        let (offset, rotation, velocity) = swing.arc.sample(t);
        Some(SwingSample { offset, rotation, velocity })
    }
}
```

#### 2.3 Input Integration

**Modify**: `shock2vr/src/virtual_hand.rs` or `input_context.rs`

```rust
// Desktop: trigger attack via mouse button
// Map to swing arc progression
// Provide synthetic velocity to MeleeWeapon script
```

---

### Phase 3: Material-Based Impact Sounds

**Goal**: Play appropriate sounds when weapons hit different materials.

#### 3.1 Material Sound Mapping

**New file**: `shock2vr/src/melee/impact_sounds.rs`

```rust
use dark::properties::PropMaterial;

pub struct ImpactSoundMapper;

impl ImpactSoundMapper {
    /// Get sound schema for weapon hitting material
    pub fn get_impact_sound(
        weapon_material: &str,
        target_material: &str,
    ) -> Option<String> {
        // Dark Engine uses material tags like "Flesh", "Metal", "Stone"
        // Map to sound schemas in snd/ folder
        match (weapon_material, target_material) {
            ("Metal", "Flesh") => Some("melee_flesh"),
            ("Metal", "Metal") => Some("melee_metal"),
            ("Metal", "Stone") => Some("melee_stone"),
            ("Energy", "Flesh") => Some("energy_flesh"),
            ("Energy", _) => Some("energy_impact"),
            _ => Some("melee_default"),
        }
    }
}
```

#### 3.2 Integration with Hit Detection

```rust
// In MeleeWeapon::update, after detecting hit:
if let Some(hit) = detect_hit(...) {
    let target_material = world
        .get::<PropMaterial>(hit.entity_id)
        .map(|m| m.0.as_str())
        .unwrap_or("Flesh");

    let sound_schema = ImpactSoundMapper::get_impact_sound(
        &self.weapon_config.weapon_material,
        target_material,
    );

    // Play sound at hit location
    effects.push(Effect::PlaySound {
        schema: sound_schema,
        position: hit.hit_point,
    });
}
```

---

### Phase 4: VR Haptic Feedback

**Goal**: Provide controller vibration on hit for tactile feedback.

#### 4.1 New Effect Type

**Modify**: `shock2vr/src/scripts/effect.rs`

```rust
pub enum Effect {
    // ... existing effects

    /// Trigger haptic feedback on VR controller
    HapticPulse {
        hand: Hand,       // Left or Right
        intensity: f32,   // 0.0 - 1.0
        duration_ms: u32, // Duration in milliseconds
    },
}
```

#### 4.2 Effect Handler

**Modify**: `shock2vr/src/mission/mission_core.rs`

```rust
Effect::HapticPulse { hand, intensity, duration_ms } => {
    // Queue haptic event for runtime to process
    // Desktop: no-op
    // VR: OpenXR haptic feedback
}
```

#### 4.3 Runtime Implementation

**Modify**: `runtimes/oculus_runtime/src/lib.rs`

```rust
// Process haptic pulse events from effect queue
// Use OpenXR haptic API to vibrate controller
```

---

### Phase 5: Debug Scene & Testing

**Goal**: Create isolated test environment for melee combat development.

#### 5.1 Debug Melee Scene

**New file**: `shock2vr/src/scenes/debug_melee.rs`

```rust
pub fn setup_debug_melee_scene(world: &mut World) {
    // Spawn player with wrench
    // Spawn various targets:
    // - Static dummy (test basic hits)
    // - Moving dummy (test tracking)
    // - Armored dummy (test hitbox multipliers)
    // - Material blocks (test impact sounds)

    // Display UI:
    // - Current weapon velocity
    // - Last damage dealt
    // - Hit location visualization
}
```

#### 5.2 Add to Debug Scene Registry

**Modify**: `shock2vr/src/scenes/mod.rs`

```rust
pub fn create_debug_scene(name: &str) -> Option<Box<dyn Scene>> {
    match name {
        "debug_melee" => Some(Box::new(DebugMeleeScene::new())),
        // ... existing scenes
    }
}
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_velocity_tracker_smoothing() { /* ... */ }

    #[test]
    fn test_damage_calculation_velocity_scaling() { /* ... */ }

    #[test]
    fn test_swing_arc_keyframe_interpolation() { /* ... */ }
}
```

### Integration Tests

- **Debug runtime + debug_melee scene**: Programmatic testing
- **Desktop runtime**: Manual testing with mouse attack
- **VR runtime**: Manual testing with physical swings

### Test Cases

1. [ ] Basic hit detection works (swing at target → damage dealt)
2. [ ] Faster swings deal more damage
3. [ ] Can't hit same entity twice in one swing
4. [ ] Hitbox multipliers apply correctly (head = 2x, etc.)
5. [ ] Desktop swing arc completes correctly
6. [ ] Impact sounds play on hit
7. [ ] VR haptics trigger on hit (VR only)
8. [ ] No damage when weapon is stationary

---

## Future Enhancements (Out of Scope for Initial Implementation)

### Blocking System

Allow weapons to block incoming melee attacks:
- Detect weapon-weapon collision
- Reduce/negate incoming damage
- Block visual/audio feedback
- Stamina cost for blocking

### Two-Handed Weapons

Support for weapons requiring both hands:
- Detect when both controllers grip weapon
- Increased damage when properly held
- Different swing mechanics
- Blocking with two hands

### Stamina System

Integrate with character stats:
- Swings consume stamina (based on velocity)
- Low stamina = reduced damage
- Stamina regeneration
- Endurance stat affects stamina pool

### Parry/Riposte

Timing-based combat mechanics:
- Parry window when blocking at right moment
- Riposte opportunity after successful parry
- Increased damage during riposte

### Weapon Durability

Weapons degrade with use:
- Track hits per weapon
- Visual degradation (texture swap)
- Damage reduction when damaged
- Repair at maintenance stations

### Combo System

Reward skilled play:
- Track swing patterns
- Bonus damage for combos
- Special moves for specific patterns

---

## Key Files Summary

| File | Purpose |
|------|---------|
| `shock2vr/src/melee/mod.rs` | Module exports |
| `shock2vr/src/melee/velocity_tracker.rs` | Track weapon velocity for damage |
| `shock2vr/src/melee/hit_detection.rs` | Swept collision detection |
| `shock2vr/src/melee/damage_calculator.rs` | Damage formula |
| `shock2vr/src/melee/weapon_config.rs` | Per-weapon stats |
| `shock2vr/src/melee/desktop_swing.rs` | Desktop swing arc system |
| `shock2vr/src/melee/impact_sounds.rs` | Material-based sounds |
| `shock2vr/src/scripts/melee_weapon.rs` | Main weapon script (enhanced) |
| `shock2vr/src/scenes/debug_melee.rs` | Debug/test scene |

---

## Dependencies & Prerequisites

- **Player damage system** (`projects/player-damage.md`): Needed if enemies can melee the player back
- **Material sound system**: May need to implement Dark Engine material→sound mapping
- **Haptic API**: Need to expose OpenXR haptics through effect system

---

## Design Decisions

These decisions were made during planning to keep the initial implementation simple:

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Weapon reach** | Fixed raycast length | All SS2 melee weapons are similar length; simplifies implementation |
| **Hit registration** | Single raycast per frame | Sufficient for most cases; swept from prev→current catches fast swings |
| **Swing detection** | Velocity threshold | Natural for VR; desktop swing arc provides synthetic velocity |
| **Multi-hit** | First entity only | Simplifies logic; future physics-based approach will naturally limit this |

---

## Long-Term Vision: Physics-Based Weapons

The initial implementation uses kinematic positioning with velocity tracking. However, the long-term goal is **physics-based weapons** that interact realistically with the world:

### Why Physics-Based?

1. **No wall clipping**: Weapon stops when hitting walls/geometry
2. **Natural multi-hit prevention**: Weapon physically blocked after first hit
3. **Weight and momentum**: Different weapons feel different
4. **Environmental interaction**: Knock objects, break glass, etc.

### Implementation Approach (Future)

```
┌─────────────────────────────────────────────────────────────┐
│              PHYSICS-BASED WEAPON (FUTURE)                  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐                                        │
│  │  VR Controller  │                                        │
│  │  (target pos)   │                                        │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           Spring/Damper Constraint                   │   │
│  │  - Weapon is dynamic rigid body                     │   │
│  │  - Connected to "target" at hand position           │   │
│  │  - Spring pulls weapon toward hand                  │   │
│  │  - Damper prevents oscillation                      │   │
│  │  - Weapon collides with world geometry              │   │
│  └─────────────────────────────────────────────────────┘   │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           Physics Collision Events                   │   │
│  │  - Weapon hits wall → stops, spring stretches       │   │
│  │  - Weapon hits enemy → damage from velocity/mass    │   │
│  │  - Weapon blocked → can't phase through             │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Challenges to Address

1. **Physics stability**: Prevent jitter when weapon constrained against geometry
2. **Tunneling**: Fast swings might skip collision detection
3. **Tuning**: Spring/damper constants for good "feel" without lag
4. **Desktop mode**: Need to drive physics from swing arc, not player input

### Migration Path

The initial kinematic implementation can be migrated incrementally:
1. Start with kinematic + velocity tracking (Phase 1)
2. Add optional physics mode behind experimental flag
3. Tune physics feel with spring/damper constants
4. Graduate to default once stable
