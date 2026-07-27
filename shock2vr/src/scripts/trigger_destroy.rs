use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TriggerDestroy;

impl TriggerDestroy {
    pub fn new() -> Self {
        Self
    }
}

impl Script for TriggerDestroy {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Slay => send_to_all_switch_links(
                world,
                entity_id,
                MessagePayload::TurnOn { from: entity_id },
            ),
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    #[test]
    fn slay_relays_turn_on_to_every_switch_link() {
        let mut world = World::new();
        let first = world.add_entity(());
        let second = world.add_entity(());
        let trigger = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 1,
                    to_entity_id: Some(WrappedEntityId(first)),
                    link: Link::SwitchLink,
                },
                ToLink {
                    to_template_id: 2,
                    to_entity_id: Some(WrappedEntityId(second)),
                    link: Link::SwitchLink,
                },
            ],
        });
        let mut script = TriggerDestroy::new();

        let effect =
            script.handle_message(trigger, &world, &PhysicsWorld::new(), &MessagePayload::Slay);
        let effects = match effect {
            Effect::Combined { effects } => effects,
            other => panic!("expected relayed messages, got {other:?}"),
        };

        assert_eq!(effects.len(), 2);
        for (effect, target) in effects.iter().zip([first, second]) {
            match effect {
                Effect::Send { msg } => {
                    assert_eq!(msg.to, target);
                    assert!(matches!(
                        msg.payload,
                        MessagePayload::TurnOn { from } if from == trigger
                    ));
                }
                other => panic!("expected TurnOn message, got {other:?}"),
            }
        }
    }

    #[test]
    fn non_slay_messages_do_not_trigger_or_remove_the_object() {
        let mut world = World::new();
        let target = world.add_entity(());
        let trigger = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 1,
                to_entity_id: Some(WrappedEntityId(target)),
                link: Link::SwitchLink,
            }],
        });
        let mut script = TriggerDestroy::new();

        let effect =
            script.handle_message(trigger, &world, &PhysicsWorld::new(), &MessagePayload::Frob);

        assert!(matches!(effect, Effect::NoEffect));
    }
}
