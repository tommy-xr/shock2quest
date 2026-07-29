use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TrapRouter {}
impl TrapRouter {
    pub fn new() -> TrapRouter {
        TrapRouter {}
    }
}
impl Script for TrapRouter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let routed = match msg {
            MessagePayload::TurnOn { .. } => MessagePayload::TurnOn { from: entity_id },
            MessagePayload::TurnOff { .. } => MessagePayload::TurnOff { from: entity_id },
            other => other.clone(),
        };
        send_to_all_switch_links(world, entity_id, routed)
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    fn forwarded(effect: Effect) -> Vec<super::super::Message> {
        Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } => Some(msg),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn router_rewrites_turn_sender_to_itself() {
        let mut world = World::new();
        let destination = world.add_entity(());
        let router = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 2,
                to_entity_id: Some(WrappedEntityId(destination)),
                link: Link::SwitchLink,
            }],
        });
        let original = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = TrapRouter::new();

        for payload in [
            MessagePayload::TurnOn { from: original },
            MessagePayload::TurnOff { from: original },
        ] {
            let messages = forwarded(script.handle_message(router, &world, &physics, &payload));
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].to, destination);
            match messages[0].payload {
                MessagePayload::TurnOn { from } | MessagePayload::TurnOff { from } => {
                    assert_eq!(from, router)
                }
                ref other => panic!("expected routed switch message, got {other:?}"),
            }
        }
    }
}
