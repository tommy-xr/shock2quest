use dark::properties::FrobFlag;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script, script_util, script_util::player_carried_items};

/// Implements the engine-level `PropFrobInfo.world_action = MOVE` behavior for
/// ordinary pickup items that do not ask an authored script to handle Frob.
///
/// `MOVE | SCRIPT` objects keep their existing scripted ownership. In
/// particular, `FrobQB` must award the Engineering circuit board's quest bit
/// and transfer it exactly once.
pub struct InternalFrobMove;

impl InternalFrobMove {
    pub fn new() -> Self {
        Self
    }
}

impl Script for InternalFrobMove {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }

        let handles_move = {
            let frob_info = world
                .borrow::<View<dark::properties::PropFrobInfo>>()
                .unwrap();
            frob_info.get(entity_id).is_ok_and(|frob_info| {
                !frob_info.world_action.contains(FrobFlag::SCRIPT)
                    && (frob_info.world_action.contains(FrobFlag::MOVE)
                        || frob_info.world_action.contains(FrobFlag::USE_AMMO))
            })
        };
        if !handles_move {
            return Effect::NoEffect;
        }

        // Key sources (`MOVE`-only access cards) also carry a derived
        // `internal_keycard` script, which registers them on the keyring and
        // consumes the object. It owns the physical fate; transferring here as
        // well would emit a backpack transfer and a destroy for one Frob.
        if script_util::is_key_source(world, entity_id) {
            return Effect::NoEffect;
        }

        // The undifferentiated Frob message is also used by inventory UI.
        // Once carried, the item's `inventory_action` belongs to its ordinary
        // script; do not re-parent it through its world MOVE action.
        if player_carried_items(world).contains(&entity_id) {
            return Effect::NoEffect;
        }

        let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
            return Effect::NoEffect;
        };
        Effect::DropEntityInfo {
            parent_entity_id: player.inventory_entity_id,
            dropped_entity_id: entity_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{FrobFlag, Link, Links, PropFrobInfo, ToLink, WrappedEntityId};
    use shipyard::World;

    use crate::{
        mission::PlayerInfo,
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::InternalFrobMove;

    fn pickup_world(
        world_action: FrobFlag,
        already_carried: bool,
    ) -> (World, shipyard::EntityId, shipyard::EntityId) {
        let mut world = World::new();
        let item = world.add_entity(PropFrobInfo {
            world_action,
            inventory_action: FrobFlag::SCRIPT,
            tool_action: FrobFlag::empty(),
        });
        let inventory = world.add_entity(if already_carried {
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(item)),
                    link: Link::Contains(0),
                }],
            }
        } else {
            Links::empty()
        });
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, item, inventory)
    }

    #[test]
    fn world_frob_moves_an_ordinary_pickup_to_the_backpack() {
        let (world, item, inventory) = pickup_world(FrobFlag::MOVE, false);

        let effect = InternalFrobMove::new().handle_message(
            item,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(
            effect,
            Effect::DropEntityInfo {
                parent_entity_id,
                dropped_entity_id,
            } if parent_entity_id == inventory && dropped_entity_id == item
        ));
    }

    /// A `MOVE`-only access card (the Rec2 crew card) also carries a derived
    /// `internal_keycard` script, which registers it and consumes the object.
    /// Transferring it here too would stash and destroy the same card.
    #[test]
    fn world_frob_leaves_a_key_source_to_its_keycard_script() {
        let (mut world, item, _inventory) = pickup_world(FrobFlag::MOVE, false);
        world.add_component(
            item,
            dark::properties::PropKeySrc(dark::properties::KeyCard {
                is_master: false,
                region_id: 32,
                lock_id: 0,
            }),
        );

        let effect = InternalFrobMove::new().handle_message(
            item,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(
            matches!(effect, Effect::NoEffect),
            "a key source must not also be transferred to the backpack, got {effect:?}"
        );
    }

    #[test]
    fn inventory_frob_does_not_reapply_the_world_move() {
        let (world, item, _inventory) = pickup_world(FrobFlag::MOVE, true);

        let effect = InternalFrobMove::new().handle_message(
            item,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn scripted_move_remains_owned_by_the_authored_script() {
        let (world, item, _inventory) = pickup_world(FrobFlag::MOVE | FrobFlag::SCRIPT, false);

        let effect = InternalFrobMove::new().handle_message(
            item,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
