use dark::properties::PropKeySrc;

use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Runtime script attached to every `PropKeySrc` object.
///
/// Frobbing a key source *registers* it on the player's keyring and consumes
/// the object: access cards are logical access, not backpack inventory. This
/// script owns the physical fate of a key source, so its sibling pickup paths
/// (`FrobQB`, `internal_frob_move`) deliberately leave the transfer alone -
/// otherwise a single Frob would both stash and destroy the same card.
pub struct KeyCardScript {}
impl KeyCardScript {
    pub fn new() -> KeyCardScript {
        KeyCardScript {}
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
            MessagePayload::Frob => {
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
    use dark::properties::{FrobFlag, KeyCard, PropFrobInfo, PropKeySrc};
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::KeyCardScript;

    fn keycard_world(world_action: FrobFlag) -> (World, shipyard::EntityId) {
        let mut world = World::new();
        let card = world.add_entity((
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 32,
                lock_id: 0,
            }),
            PropFrobInfo {
                world_action,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        (world, card)
    }

    /// Rec2 crew card 996 is a grabbable `MOVE` key source. Frobbing it grants
    /// the keyring entry and consumes the object - the card is access, not a
    /// backpack item.
    #[test]
    fn frobbing_a_move_keycard_registers_it_and_removes_the_object() {
        let (world, card) = keycard_world(FrobFlag::MOVE);

        let effects = Effect::flatten(vec![KeyCardScript::new().handle_message(
            card,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::AcquireKeyCard { .. })),
            "key access must be granted, got {effects:?}"
        );
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == card)
            ),
            "the registered card must be consumed, got {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DropEntityInfo { .. })),
            "a registered card must never be stashed in the backpack, got {effects:?}"
        );
    }

    /// Use-only key sources (card slots, non-grabbable readers) behave the
    /// same: register, then consume.
    #[test]
    fn frobbing_a_use_only_key_source_registers_and_consumes_it() {
        let (world, card) = keycard_world(FrobFlag::SCRIPT);

        let effects = Effect::flatten(vec![KeyCardScript::new().handle_message(
            card,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::AcquireKeyCard { .. }))
        );
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == card)
        ));
    }
}
