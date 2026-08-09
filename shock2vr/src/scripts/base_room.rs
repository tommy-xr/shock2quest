use shipyard::{EntityId, UniqueView, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

/// The authored `BaseRoom` behavior used by `EnterRoom` objects.
pub struct BaseRoom;

impl BaseRoom {
    pub fn new() -> Self {
        Self
    }
}

impl Script for BaseRoom {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let player = world.borrow::<UniqueView<PlayerInfo>>();
        match (msg, player) {
            (MessagePayload::SensorBeginIntersect { with }, Ok(player))
                if *with == player.entity_id =>
            {
                send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                )
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    #[test]
    fn player_entry_turns_on_authored_switch_links() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        let destination = world.add_entity(());
        let room = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 29,
                to_entity_id: Some(WrappedEntityId(destination)),
                link: Link::SwitchLink,
            }],
        });

        let effect = BaseRoom::new().handle_message(
            room,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::SensorBeginIntersect { with: player },
        );
        let messages: Vec<_> = Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } => Some(msg),
                _ => None,
            })
            .collect();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].to, destination);
        assert!(matches!(
            messages[0].payload,
            MessagePayload::TurnOn { from } if from == room
        ));
    }

    #[test]
    fn non_player_and_exit_intersections_do_not_activate_the_room() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        let other = world.add_entity(());
        let destination = world.add_entity(());
        let room = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 29,
                to_entity_id: Some(WrappedEntityId(destination)),
                link: Link::SwitchLink,
            }],
        });
        let physics = PhysicsWorld::new();
        let mut script = BaseRoom::new();

        for payload in [
            MessagePayload::SensorBeginIntersect { with: other },
            MessagePayload::SensorEndIntersect { with: player },
        ] {
            assert!(matches!(
                script.handle_message(room, &world, &physics, &payload),
                Effect::NoEffect
            ));
        }
    }
}
