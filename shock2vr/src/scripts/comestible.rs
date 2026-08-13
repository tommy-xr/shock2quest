use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::player_carried_items};

/// Retail food and drink inventory use.
///
/// The shipped 25AE `allobjs` module handles `FrobInvEnd`: it adds one hit
/// point to the frobber, plays the object's authored Activate environmental
/// sound, and destroys the object. The port's `Frob` payload is shared by
/// world pickup and inventory use, so the carried-item check preserves the
/// object's inherited world `MOVE` action while selecting the authored
/// inventory `SCRIPT` path.
pub struct Comestible;

impl Comestible {
    pub fn new() -> Self {
        Self
    }
}

impl Script for Comestible {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) || !player_carried_items(world).contains(&entity_id)
        {
            return Effect::NoEffect;
        }

        Effect::UseComestible {
            entity_id,
            hit_points: 1,
        }
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

    use super::Comestible;

    fn world_with_food(carried: bool) -> (World, shipyard::EntityId) {
        let mut world = World::new();
        let food = world.add_entity(());
        let inventory = world.add_entity(if carried {
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(food)),
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
        (world, food)
    }

    #[test]
    fn carried_food_requests_one_atomic_retail_use() {
        let (world, food) = world_with_food(true);

        let effect = Comestible::new().handle_message(
            food,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(
            effect,
            Effect::UseComestible {
                entity_id,
                hit_points: 1,
            } if entity_id == food
        ));
    }

    #[test]
    fn world_frob_remains_owned_by_inherited_move_action() {
        let (world, food) = world_with_food(false);

        let effect = Comestible::new().handle_message(
            food,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn unrelated_messages_do_nothing() {
        let (world, food) = world_with_food(true);

        let effect = Comestible::new().handle_message(
            food,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: food },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
