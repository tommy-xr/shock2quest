use std::time::Duration;

use dark::properties::PropAnimLight;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

const STATE_KEY: &str = "shock2vr.base_light";

/// Retail `BaseLight`: switch the object's authored animated-light contribution
/// in response to the same TurnOn/TurnOff messages used by buttons and traps.
#[derive(Default, Serialize, Deserialize)]
pub struct BaseLight {
    // Some(ZERO) means destruction was already requested. Duplicate Slay
    // messages must neither restart the timer nor relight a dying fixture.
    destroy_in: Option<Duration>,
}

impl BaseLight {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Script for BaseLight {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if self.destroy_in.is_some() {
            return Effect::NoEffect;
        }
        if matches!(msg, MessagePayload::Slay) {
            // Retail BaseLight::OnSlain turns the light down, then schedules
            // ReallySlay after 100 ms (allobjs x64 0x18000d6d0). Rad FX also
            // inherits BaseLight on particle-only objects: they need the
            // delayed destruction even without an AnimLight property.
            self.destroy_in = Some(Duration::from_millis(100));
            return world
                .borrow::<View<PropAnimLight>>()
                .ok()
                .and_then(|lights| lights.get(entity_id).ok().map(|l| l.minimum_intensity()))
                .map_or(Effect::NoEffect, |intensity| Effect::SetAnimatedLight {
                    entity_id,
                    intensity,
                    inactive: true,
                });
        }
        let lights = world.borrow::<View<PropAnimLight>>().unwrap();
        let Ok(light) = lights.get(entity_id) else {
            return Effect::NoEffect;
        };

        match msg {
            MessagePayload::TurnOn { .. } => Effect::SetAnimatedLight {
                entity_id,
                intensity: 1.0,
                inactive: false,
            },
            MessagePayload::TurnOff { .. } => {
                let intensity = light.turn_off_intensity();
                Effect::SetAnimatedLight {
                    entity_id,
                    intensity,
                    inactive: intensity <= f32::EPSILON,
                }
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if let Some(remaining) = &mut self.destroy_in {
            if !remaining.is_zero() {
                *remaining = remaining.saturating_sub(time.elapsed);
                if remaining.is_zero() {
                    return Effect::DestroyEntity { entity_id };
                }
            }
        }
        Effect::NoEffect
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        saved: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = saved.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cgmath::Vector3;
    use dark::properties::{AnimLightMode, PropAnimLight};
    use shipyard::World;

    use crate::physics::PhysicsWorld;

    use super::*;

    fn max_light() -> PropAnimLight {
        PropAnimLight {
            offset: Vector3::new(0.0, 0.0, 0.0),
            cell_index: 287,
            hit_cells: 11,
            light_number: 195,
            mode: AnimLightMode::MaxBrightness,
            brighten_time_ms: 63,
            dim_time_ms: 63,
            min_brightness: 0.0,
            max_brightness: 150.0,
            brightness: Some(150.0),
            rising: false,
            countdown_ms: 0,
            countdown_fraction_ms: 0.0,
            inactive: false,
            radius: 50.0,
        }
    }

    #[test]
    fn switched_max_light_emits_persistent_render_effects() {
        let mut world = World::new();
        let entity = world.add_entity(max_light());
        let physics = PhysicsWorld::new();
        let mut script = BaseLight::new();

        let off = script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::TurnOff { from: entity },
        );
        assert!(matches!(
            off,
            Effect::SetAnimatedLight {
                entity_id,
                intensity,
                inactive: true,
            } if entity_id == entity && intensity == 0.0
        ));

        let on = script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity },
        );
        assert!(matches!(
            on,
            Effect::SetAnimatedLight {
                entity_id,
                intensity,
                inactive: false,
            } if entity_id == entity && intensity == 1.0
        ));
    }

    #[test]
    fn ignores_unrelated_messages_and_entities_without_anim_light() {
        let mut world = World::new();
        let light = world.add_entity(max_light());
        let plain = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = BaseLight::new();

        assert!(matches!(
            script.handle_message(light, &world, &physics, &MessagePayload::Frob),
            Effect::NoEffect
        ));
        assert!(matches!(
            script.handle_message(
                plain,
                &world,
                &physics,
                &MessagePayload::TurnOn { from: light }
            ),
            Effect::NoEffect
        ));
    }

    #[test]
    fn slay_extinguishes_then_destroys_once_and_preserves_timer_on_load() {
        let mut world = World::new();
        let mut light = max_light();
        light.mode = AnimLightMode::Alternate;
        // Shutdown must freeze even a nonzero authored minimum, rather than
        // restarting its alternating animation after the dark phase.
        light.min_brightness = 30.0;
        let entity = world.add_entity(light);
        let physics = PhysicsWorld::new();
        let mut script = BaseLight::new();
        assert!(matches!(
            script.handle_message(entity, &world, &physics, &MessagePayload::Slay),
            Effect::SetAnimatedLight { intensity, inactive: true, .. }
                if (intensity - 0.2).abs() < 1e-6
        ));
        let tick = |script: &mut BaseLight, millis| {
            script.update(
                entity,
                &world,
                &physics,
                &Time {
                    elapsed: Duration::from_millis(millis),
                    total: Duration::ZERO,
                },
            )
        };
        assert!(matches!(tick(&mut script, 40), Effect::NoEffect));
        for message in [
            MessagePayload::Slay,
            MessagePayload::TurnOn { from: entity },
        ] {
            assert!(matches!(
                script.handle_message(entity, &world, &physics, &message),
                Effect::NoEffect
            ));
        }
        let bytes = serde_json::to_vec(&script.save_state().unwrap()).unwrap();
        let mut restored = BaseLight::new();
        restored
            .restore_state(
                &serde_json::from_slice(&bytes).unwrap(),
                &ScriptRestoreContext::new(&HashMap::new()),
            )
            .unwrap();
        assert!(matches!(tick(&mut restored, 0), Effect::NoEffect));
        assert!(matches!(tick(&mut restored, 59), Effect::NoEffect));
        assert!(
            matches!(tick(&mut restored, 1), Effect::DestroyEntity { entity_id } if entity_id == entity)
        );
        assert!(matches!(tick(&mut restored, 100), Effect::NoEffect));
    }

    #[test]
    fn particle_only_rad_fx_also_gets_delayed_destruction() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = BaseLight::new();
        assert!(matches!(
            script.handle_message(entity, &world, &physics, &MessagePayload::Slay),
            Effect::NoEffect
        ));
        assert!(matches!(
            script.update(entity, &world, &physics, &Time {
                elapsed: Duration::from_millis(100),
                total: Duration::ZERO,
            }),
            Effect::DestroyEntity { entity_id } if entity_id == entity
        ));
    }
}
