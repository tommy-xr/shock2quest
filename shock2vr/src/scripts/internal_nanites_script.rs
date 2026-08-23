use dark::properties::PropStackCount;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Retail's fallback "linebeep" cue reused as the nanite pickup sound. There
/// is no dedicated retail nanite-pickup cue identified in this port's sound
/// data yet, so this mirrors the `LOG_PICKUP_SOUND` fallback used by
/// `scripts::gui::media` for the same reason - see that module for the
/// precedent.
const NANITE_PICKUP_SOUND: &str = "linebeep";

/// Derived script attached to every world nanite pickup (see
/// `mission::entity_creator` for the identification), mirroring
/// `ExpCookie`/`internal_keycard`'s "collect directly into a player stat"
/// shape. Frobbing (or, via `virtual_hand`/`flat_player_controller` routing,
/// squeezing) a world nanite pile awards its stack count straight to the
/// player's persistent nanite balance and removes the object - it never
/// enters the inventory grid or the physical hand.
pub struct InternalNanitesScript {}

impl InternalNanitesScript {
    pub fn new() -> InternalNanitesScript {
        InternalNanitesScript {}
    }
}

impl Script for InternalNanitesScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                let amount = world
                    .borrow::<View<PropStackCount>>()
                    .ok()
                    .and_then(|stacks| stacks.get(entity_id).ok().map(|s| s.0))
                    .filter(|&n| n > 0)
                    .unwrap_or(1);

                Effect::Combined {
                    effects: vec![
                        Effect::AwardNanites { amount },
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: NANITE_PICKUP_SOUND.to_owned(),
                            spatial: false,
                        },
                        Effect::DestroyEntity { entity_id },
                    ],
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::PropStackCount;

    #[test]
    fn frob_awards_nanites_plays_a_sound_and_destroys_the_pickup() {
        let mut world = World::new();
        let entity = world.add_entity(PropStackCount(20));

        let effect = InternalNanitesScript::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        let Effect::Combined { effects } = effect else {
            panic!("expected a combined effect, got {effect:?}");
        };
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount } if *amount == 20))
        );
        assert!(effects.iter().any(
            |e| matches!(e, Effect::PlaySound { source: Some(source), .. } if *source == entity)
        ));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == entity))
        );
    }

    #[test]
    fn frob_without_a_stack_count_defaults_to_one() {
        let mut world = World::new();
        let entity = world.add_entity(());

        let effect = InternalNanitesScript::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        let Effect::Combined { effects } = effect else {
            panic!("expected a combined effect, got {effect:?}");
        };
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount } if *amount == 1))
        );
    }

    #[test]
    fn non_frob_messages_are_ignored() {
        let mut world = World::new();
        let entity = world.add_entity(PropStackCount(5));

        let effect = InternalNanitesScript::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: entity },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
