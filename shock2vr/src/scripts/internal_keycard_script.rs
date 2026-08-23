use dark::properties::PropKeySrc;

use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct KeyCardScript {
    // The entity is destroyed the same frame it is first collected (effects
    // apply after the whole message batch), so a second Frob delivered in
    // that same batch - e.g. both a panel squeeze and a direct world squeeze
    // routing to the same entity in one update - would otherwise still see it
    // alive and push a duplicate credential while only one DestroyEntity is
    // ever queued (`add_key_card` is a bare `Vec::push`, not idempotent).
    // Latch in-memory on the first award, mirroring
    // `InternalNanitesScript::collected`; the entity is gone by the next
    // frame, so this never needs to survive a save.
    collected: bool,
}
impl KeyCardScript {
    pub fn new() -> KeyCardScript {
        KeyCardScript { collected: false }
    }
}

impl Script for KeyCardScript {
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

                let v_keycard_src = world.borrow::<View<PropKeySrc>>().unwrap();
                let maybe_keycard = v_keycard_src.get(entity_id);
                let acquire_key_card = {
                    if let Ok(key_card) = maybe_keycard {
                        Effect::AcquireKeyCard {
                            key_card: key_card.0.clone(),
                        }
                    } else {
                        Effect::NoEffect
                    }
                };

                let destroy_self = Effect::DestroyEntity { entity_id };
                Effect::Combined {
                    effects: vec![acquire_key_card, destroy_self],
                }
            }
            // Does turn off need to be done for email?
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::KeyCard;

    fn keycard() -> KeyCard {
        KeyCard {
            is_master: false,
            region_id: 128,
            lock_id: 0,
        }
    }

    /// Negative-first regression: the destroy effect only applies after the
    /// whole message batch, so a second Frob in the same batch - e.g. a panel
    /// squeeze and a direct world squeeze both landing on the same card -
    /// still finds the entity alive. Without the `collected` latch, this
    /// would push a duplicate credential (`add_key_card` is a bare
    /// `Vec::push`).
    #[test]
    fn a_second_frob_in_the_same_batch_acquires_nothing_more() {
        let mut world = World::new();
        let entity = world.add_entity(PropKeySrc(keycard()));
        let mut script = KeyCardScript::new();

        let first =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);
        let second =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);

        let acquire_count = |effect: &Effect| -> usize {
            match effect {
                Effect::Combined { effects } => effects
                    .iter()
                    .filter(|e| matches!(e, Effect::AcquireKeyCard { .. }))
                    .count(),
                Effect::AcquireKeyCard { .. } => 1,
                _ => 0,
            }
        };

        assert_eq!(
            acquire_count(&first),
            1,
            "first Frob should acquire the credential"
        );
        assert_eq!(
            acquire_count(&second),
            0,
            "second Frob in the same batch must not acquire the credential again"
        );
        assert!(
            matches!(second, Effect::NoEffect),
            "second Frob should be a no-op, got {second:?}"
        );
    }

    #[test]
    fn frob_acquires_the_credential_and_destroys_the_card() {
        let mut world = World::new();
        let entity = world.add_entity(PropKeySrc(keycard()));

        let effect = KeyCardScript::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        let Effect::Combined { effects } = effect else {
            panic!("expected a combined effect, got {effect:?}");
        };
        assert!(effects.iter().any(
            |e| matches!(e, Effect::AcquireKeyCard { key_card } if key_card.region_id == 128)
        ));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == entity))
        );
    }
}
