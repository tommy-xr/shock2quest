use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::player_carried_items};

/// Remove every entity the player is carrying when an authored training-exit
/// trap activates. Earth composes this mission-local script with the inherited
/// `TrapTeleportPlayer`; returning destruction effects here lets both scripts
/// run through the normal effect pipeline on the same `TurnOn`.
pub struct VaporizeInventory;

impl VaporizeInventory {
    pub fn new() -> Self {
        Self
    }
}

impl Script for VaporizeInventory {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }

        Effect::combine(
            player_carried_items(world)
                .into_iter()
                .map(|entity_id| Effect::DestroyEntity { entity_id })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};
    use shipyard::World;

    use crate::{
        mission::PlayerInfo,
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::VaporizeInventory;

    fn contains(entity: shipyard::EntityId) -> ToLink {
        ToLink {
            to_template_id: 0,
            to_entity_id: Some(WrappedEntityId(entity)),
            link: Link::Contains(0),
        }
    }

    #[test]
    fn turn_on_destroys_hands_backpack_and_nested_carried_entities_once() {
        let mut world = World::new();
        let player = world.add_entity(());
        let nested_equipment = world.add_entity(Links::empty());
        let left_hand = world.add_entity(Links {
            to_links: vec![contains(nested_equipment)],
        });
        let right_hand = world.add_entity(Links::empty());
        let backpack_item = world.add_entity(Links::empty());
        let inventory = world.add_entity(Links {
            // Duplicate the left hand deliberately: malformed ownership must
            // not emit two destruction effects for one entity.
            to_links: vec![contains(backpack_item), contains(left_hand)],
        });
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: Some(left_hand),
            right_hand_entity_id: Some(right_hand),
            inventory_entity_id: inventory,
        });

        let effect = VaporizeInventory::new().handle_message(
            player,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: player },
        );
        let destroyed: Vec<_> = Effect::flatten(vec![effect])
            .into_iter()
            .map(|effect| match effect {
                Effect::DestroyEntity { entity_id } => entity_id,
                other => panic!("expected only DestroyEntity, got {other:?}"),
            })
            .collect();

        assert_eq!(
            destroyed,
            vec![left_hand, nested_equipment, right_hand, backpack_item]
        );
        assert!(
            !destroyed.contains(&inventory),
            "the backpack container survives"
        );
        assert!(!destroyed.contains(&player), "the player survives");
    }

    #[test]
    fn unrelated_messages_leave_inventory_untouched() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let effect = VaporizeInventory::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
