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
pub struct InternalNanitesScript {
    // The entity is destroyed the same frame it is first collected (effects
    // apply after the whole message batch), so a second Frob delivered in
    // that same batch - both VR hands squeezing the same pile, or trigger and
    // squeeze from different hands - would otherwise still see it alive and
    // award nanites twice while only one DestroyEntity is ever queued. Latch
    // in-memory on the first award; the entity is gone by the next frame, so
    // this never needs to survive a save.
    collected: bool,
}

impl InternalNanitesScript {
    pub fn new() -> InternalNanitesScript {
        InternalNanitesScript { collected: false }
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
            MessagePayload::Frob if !self.collected => {
                self.collected = true;

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

    /// Negative-first regression: the destroy effect only applies after the
    /// whole message batch, so a second Frob in the same batch - both VR
    /// hands squeezing the same pile, or trigger and squeeze from different
    /// hands - still finds the entity alive. Without the `collected` latch,
    /// this would award nanites twice for one pickup.
    #[test]
    fn a_second_frob_in_the_same_batch_awards_nothing_more() {
        let mut world = World::new();
        let entity = world.add_entity(PropStackCount(20));
        let mut script = InternalNanitesScript::new();

        let first =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);
        let second =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);

        let award_count = |effect: &Effect| -> usize {
            match effect {
                Effect::Combined { effects } => effects
                    .iter()
                    .filter(|e| matches!(e, Effect::AwardNanites { .. }))
                    .count(),
                Effect::AwardNanites { .. } => 1,
                _ => 0,
            }
        };

        assert_eq!(award_count(&first), 1, "first Frob should award nanites");
        assert_eq!(
            award_count(&second),
            0,
            "second Frob in the same batch must not award nanites again"
        );
        assert!(
            matches!(second, Effect::NoEffect),
            "second Frob should be a no-op, got {second:?}"
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
