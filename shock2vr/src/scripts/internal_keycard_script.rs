use dark::properties::PropKeySrc;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script};

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

                // Physical cards remain legible in the backpack after they
                // grant access. The injected keycard script owns both effects
                // so pickup cannot grant access without completing transfer.
                let physical_fate = if crate::virtual_hand::can_grab_item(world, entity_id) {
                    world
                        .borrow::<UniqueView<PlayerInfo>>()
                        .map(|player| Effect::DropEntityInfo {
                            parent_entity_id: player.inventory_entity_id,
                            dropped_entity_id: entity_id,
                        })
                        .unwrap_or(Effect::DestroyEntity { entity_id })
                } else {
                    Effect::DestroyEntity { entity_id }
                };

                Effect::combine(vec![acquire_key_card, physical_fate])
            }
            // Does turn off need to be done for email?
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{FrobFlag, KeyCard, Links, PropFrobInfo, PropKeySrc};
    use shipyard::World;

    use crate::{
        mission::PlayerInfo,
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::KeyCardScript;

    fn keycard_world(world_action: FrobFlag) -> (World, shipyard::EntityId, shipyard::EntityId) {
        let mut world = World::new();
        let keycard = world.add_entity((
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 8192,
                lock_id: 0,
            }),
            PropFrobInfo {
                world_action,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, keycard, inventory)
    }

    #[test]
    fn frobbing_a_takeable_keycard_grants_access_and_stores_it() {
        let (world, keycard, inventory) = keycard_world(FrobFlag::MOVE);

        let effects = Effect::flatten(vec![KeyCardScript::new().handle_message(
            keycard,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::AcquireKeyCard { key_card }
                if key_card.region_id == 8192 && key_card.lock_id == 0
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::DropEntityInfo {
                parent_entity_id,
                dropped_entity_id,
            } if *parent_entity_id == inventory && *dropped_entity_id == keycard
        )));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { .. }))
        );
    }

    #[test]
    fn frobbing_a_use_only_keycard_still_consumes_it() {
        let (world, keycard, _inventory) = keycard_world(FrobFlag::SCRIPT);

        let effects = Effect::flatten(vec![KeyCardScript::new().handle_message(
            keycard,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )]);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::AcquireKeyCard { .. }))
        );
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::DestroyEntity { entity_id } if *entity_id == keycard
        )));
    }
}
