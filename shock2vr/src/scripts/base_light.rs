use dark::properties::PropAnimLight;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Retail `BaseLight`: switch the object's authored animated-light contribution
/// in response to the same TurnOn/TurnOff messages used by buttons and traps.
pub struct BaseLight;

impl BaseLight {
    pub fn new() -> Self {
        Self
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
}

#[cfg(test)]
mod tests {
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
            rising: false,
            countdown_ms: 0,
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
}
