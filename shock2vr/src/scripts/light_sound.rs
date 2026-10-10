use engine::audio::AudioHandle;
use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::play_environmental_sound};

/// The authored lighttype selects the sound schema (e.g. medclax -> clax1hi).
/// Play on the bright edge, rather than every frame during the bright interval.
pub struct LightSoundOn;

impl Script for LightSoundOn {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::LightChange {
                previous_intensity,
                intensity,
            } if *previous_intensity < 1.0 && *intensity >= 1.0 => {
                play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new())
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Matrix4, vec3};
    use dark::properties::PropClassTag;

    use crate::runtime_props::RuntimePropTransform;

    use super::*;

    #[test]
    fn bright_edge_uses_authored_schema_tags_and_world_position_only_once() {
        let mut world = World::new();
        let position = vec3(1.0, 2.0, 3.0);
        let entity = world.add_entity((
            PropClassTag::from_string("lighttype medclax"),
            RuntimePropTransform(Matrix4::from_translation(position)),
        ));
        let physics = PhysicsWorld::new();
        let mut script = LightSoundOn;
        let effect = script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::LightChange {
                previous_intensity: 0.0,
                intensity: 1.0,
            },
        );
        let Effect::PlayEnvironmentalSound {
            query,
            position: actual_position,
            ..
        } = effect
        else {
            panic!("bright edge should play the klaxon");
        };
        assert_eq!(actual_position, position);
        assert_eq!(
            query.tag_values(),
            vec![
                ("event".to_owned(), "activate".to_owned()),
                ("lighttype".to_owned(), "medclax".to_owned()),
            ]
        );
        for (previous_intensity, intensity) in [(1.0, 1.0), (1.0, 0.0), (0.0, 0.0)] {
            assert!(matches!(
                script.handle_message(
                    entity,
                    &world,
                    &physics,
                    &MessagePayload::LightChange {
                        previous_intensity,
                        intensity
                    }
                ),
                Effect::NoEffect
            ));
        }
    }
}
