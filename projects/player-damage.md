# Player Damage

This document describes the implementation plan for player damage system - currently, enemies cannot damage the player.

## Core System Architecture Analysis

### Current State:
- **PlayerInfo**: Simple struct with position, rotation, entity_id, hand/inventory references (`shock2vr/src/mission/mission_core.rs:94`)
- **Damage System**: Uses `MessagePayload::Damage { amount: f32 }` → `Effect::AdjustHitPoints` → modifies `PropHitPoints.hit_points`
- **AI Entities**: Already handle damage via `animated_monster_ai.rs:280-286`
- **Projectiles**: Use raycasting, send damage messages (`internal_fast_projectile.rs:74-79`)
- **Animation Flags**: Trigger actions via `AnimationFlagTriggered` messages (`scripts/mod.rs:114`)

## Implementation Plan

### Phase 1: Player Health Infrastructure & Testing

#### 1.1 Create Debug Damage Scene
```rust
// New file: shock2vr/src/scenes/debug_damage.rs
pub struct DebugDamageScene {
    // Spawn: Pipe Hybrid, Shotgun Hybrid, Midwife, Explosive Barrel
    // Player positioned optimally for testing all damage types
    // UI showing current health, last damage taken
    // Keyboard shortcuts to reset health, trigger specific attacks
}
```

#### 1.2 Add Player Health Property
```rust
// Location: shock2vr/src/mission/entity_creator.rs
// When creating player entity, add PropHitPoints + PropMaxHitPoints
if template_id == PLAYER_TEMPLATE_ID {
    world.add_component(entity_id, PropHitPoints { hit_points: 100 })?;
    world.add_component(entity_id, PropMaxHitPoints { hit_points: 100 })?;
}
```

**Decision**: Use `PropHitPoints` on player entity (not PlayerInfo) for consistency with existing damage infrastructure and save/load system.

#### 1.3 Create Player Script
```rust
// New file: shock2vr/src/scripts/player_script.rs
pub struct PlayerScript {
    pub invincibility_timer: f32, // Brief invincibility after taking damage
}

impl Script for PlayerScript {
    fn handle_message(&mut self, entity_id: EntityId, world: &World, _physics: &PhysicsWorld, msg: &MessagePayload) -> Effect {
        match msg {
            MessagePayload::Damage { amount } => {
                if self.invincibility_timer <= 0.0 {
                    self.invincibility_timer = 0.5; // 500ms invincibility
                    Effect::combine(vec![
                        Effect::AdjustHitPoints { entity_id, delta: -(*amount as i32) },
                        Effect::PlayerDamageResponse { amount: *amount }, // New effect for UI/sound
                    ])
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect
        }
    }

    fn update(&mut self, _entity_id: EntityId, _world: &World, _physics: &PhysicsWorld, time: &Time) -> Effect {
        if self.invincibility_timer > 0.0 {
            self.invincibility_timer -= time.elapsed.as_secs_f32();
        }
        Effect::NoEffect
    }
}
```

**Testing Requirements:**
- Verify player takes damage from each enemy type
- Test melee attacks (close range)
- Test projectile attacks (ranged)
- Test explosion damage (area of effect)
- Validate invincibility frames work correctly
- Ensure health persists through save/load

### Phase 2: Damage Sources Implementation

#### 2.1 Melee Attack Enhancement
```rust
// Location: shock2vr/src/scripts/ai/animated_monster_ai.rs:404
// Enhance existing animation flag handling:
MessagePayload::AnimationFlagTriggered { motion_flags } => {
    if motion_flags.contains(MotionFlags::FIRE) {
        fire_ranged_projectile(world, entity_id)
    } else if motion_flags.contains(MotionFlags::MELEE_HIT) { // New flag
        perform_melee_attack(world, entity_id) // New function
    } else if motion_flags.contains(MotionFlags::UNK7) {
        // ... existing die logic
    }
}

fn perform_melee_attack(world: &World, entity_id: EntityId) -> Effect {
    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_positions = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(attacker_pos) = v_positions.get(entity_id) {
        let distance = (attacker_pos.position - u_player.pos).magnitude();
        const MELEE_RANGE: f32 = 8.0 / SCALE_FACTOR; // From melee_attack_behavior.rs:51

        if distance < MELEE_RANGE {
            // Check if player is in attack arc (45 degree cone)
            let to_player = (u_player.pos - attacker_pos.position).normalize();
            let attacker_forward = get_entity_forward(world, entity_id);
            let angle = attacker_forward.dot(to_player).acos();

            if angle < std::f32::consts::PI / 4.0 { // 45 degree cone
                return Effect::Send {
                    msg: Message {
                        to: u_player.entity_id,
                        payload: MessagePayload::Damage { amount: 15.0 }, // Base melee damage
                    }
                };
            }
        }
    }
    Effect::NoEffect
}
```

#### 2.2 Projectile System Enhancement
```rust
// Location: shock2vr/src/scripts/internal_fast_projectile.rs:52
// Modify projectile_ray_cast to include player hit detection
fn enhanced_projectile_ray_cast(/*...*/) -> Option<RayCastResult> {
    // Current raycast logic...

    // Additional check for player entity hit
    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let player_sphere = Sphere::new(u_player.pos, PLAYER_RADIUS);

    if ray_intersects_sphere(start_point, forward, distance, &player_sphere) {
        return Some(RayCastResult {
            hit_point: intersection_point,
            maybe_entity_id: Some(u_player.entity_id),
            // ... other fields
        });
    }

    maybe_hit_spot
}
```

#### 2.3 Explosion Damage System
```rust
// New file: shock2vr/src/scripts/explosion_damage.rs
pub struct ExplosionDamageScript {
    pub damage_amount: f32,
    pub damage_radius: f32,
    pub has_triggered: bool,
}

impl Script for ExplosionDamageScript {
    fn update(&mut self, entity_id: EntityId, world: &World, _physics: &PhysicsWorld, _time: &Time) -> Effect {
        if !self.has_triggered {
            self.has_triggered = true;

            let v_positions = world.borrow::<View<PropPosition>>().unwrap();
            let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();

            if let Ok(explosion_pos) = v_positions.get(entity_id) {
                let distance = (explosion_pos.position - u_player.pos).magnitude();

                if distance < self.damage_radius {
                    // Linear damage falloff
                    let damage_multiplier = 1.0 - (distance / self.damage_radius);
                    let final_damage = self.damage_amount * damage_multiplier;

                    return Effect::Send {
                        msg: Message {
                            to: u_player.entity_id,
                            payload: MessagePayload::Damage { amount: final_damage },
                        }
                    };
                }
            }
        }
        Effect::NoEffect
    }
}

// Register on "HE Explosion" template creation
```

### Phase 3: Player Feedback Systems

#### 3.1 New Effect Types
```rust
// Location: shock2vr/src/scripts/effect.rs
pub enum Effect {
    // ... existing effects
    PlayerDamageResponse {
        amount: f32,
    },
    PlayerHealthChanged {
        current: i32,
        max: i32,
    },
}
```

#### 3.2 Health UI Integration
```rust
// Location: shock2vr/src/hud/ (new health_display.rs)
// Visual health bar, damage flash effects, death screen
```

### Phase 4: Advanced Features

#### 4.1 Damage Types & Resistance
```rust
#[derive(Clone, Debug)]
pub enum DamageType {
    Physical,
    Energy,
    Psi,
    Toxic,
}

#[derive(Clone, Debug)]
pub struct TypedDamage {
    pub amount: f32,
    pub damage_type: DamageType,
}
```

#### 4.2 Player Death Handling
```rust
// Extend PlayerScript to handle death (hit_points <= 0)
// Trigger respawn system or game over screen
```

## Key System Integration Points

1. **Player Entity** ↔ **PropHitPoints**: Standard entity-component relationship
2. **AI Behavior** → **Animation Flags** → **Damage Messages**: Indirect coupling via messaging
3. **Projectile Scripts** → **Raycast System** → **Player Entity**: Direct spatial query
4. **Explosion Scripts** → **Spatial Proximity** → **Player Entity**: Distance-based damage
5. **Save/Load System**: Already handles PropHitPoints via shipyard serialization

## Testing Strategy

### Integration Testing
The `debug_damage.rs` scene provides comprehensive integration testing for:
- All three damage types (melee, projectile, explosion)
- Visual confirmation of health changes
- Real-time testing in VR environment
- Easy reset and retry functionality

### System Integration Points
- **Entity Creation**: `mission/entity_creator.rs:populate_entity()`
- **Damage Flow**: `scripts/mod.rs` → `mission_core.rs:handle_effect()`
- **Animation System**: `mission_core.rs:724` (animation flag dispatch)