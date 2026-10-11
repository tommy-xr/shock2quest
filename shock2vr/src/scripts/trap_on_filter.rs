use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

/// Relay only TurnOn over SwitchLinks, matching retail allobjs.osm.
pub struct TrapOnFilter {}
impl TrapOnFilter {
    pub fn new() -> TrapOnFilter {
        TrapOnFilter {}
    }
}

impl Script for TrapOnFilter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                send_to_all_switch_links(world, entity_id, msg.clone())
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::ScriptWorld;
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    #[test]
    fn registered_filters_relay_only_their_named_edge_to_every_switch_link() {
        let mut world = World::new();
        let first = world.add_entity(());
        let second = world.add_entity(());
        let sender = world.add_entity(());
        let filter = world.add_entity(Links {
            to_links: [first, second]
                .into_iter()
                .map(|target| ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                })
                .collect(),
        });
        let physics = PhysicsWorld::new();

        // Resolve by authored name: this catches the original registry alias
        // as well as a handler listening for the wrong edge.
        for (name, passes_on) in [("TrapOnFilter", true), ("TrapOffFilter", false)] {
            let mut script = ScriptWorld::create_script(name.to_owned());
            for input in [
                MessagePayload::TurnOn { from: sender },
                MessagePayload::TurnOff { from: sender },
                MessagePayload::Frob,
            ] {
                let should_relay = match input {
                    MessagePayload::TurnOn { .. } => passes_on,
                    MessagePayload::TurnOff { .. } => !passes_on,
                    _ => false,
                };
                let effects = Effect::flatten(vec![
                    script.handle_message(filter, &world, &physics, &input),
                ]);
                let mut recipients = Vec::new();
                for effect in effects {
                    match effect {
                        Effect::NoEffect => {}
                        Effect::Send { msg } => {
                            assert!(should_relay, "{name} forwarded a rejected message");
                            match msg.payload {
                                MessagePayload::TurnOn { from } if passes_on => {
                                    assert_eq!(from, sender);
                                }
                                MessagePayload::TurnOff { from } if !passes_on => {
                                    assert_eq!(from, sender);
                                }
                                _ => panic!("{name} changed the message polarity"),
                            }
                            recipients.push(msg.to);
                        }
                        _ => panic!("{name} emitted an unexpected effect"),
                    }
                }
                assert_eq!(
                    recipients,
                    if should_relay {
                        vec![first, second]
                    } else {
                        vec![]
                    },
                    "{name} did not relay to the expected targets"
                );
            }
        }
    }
}
