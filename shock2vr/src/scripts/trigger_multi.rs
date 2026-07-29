use std::collections::HashSet;

use dark::properties::{Link, Links, ToLink};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, ViewMut, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TriggerMulti {
    entities_left_to_trigger: HashSet<EntityId>,
}
impl TriggerMulti {
    pub fn new() -> TriggerMulti {
        TriggerMulti {
            entities_left_to_trigger: HashSet::new(),
        }
    }
}
impl Script for TriggerMulti {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_links = world.borrow::<View<Links>>().unwrap();

        for (producer_entity_id, link) in v_links.iter().with_id() {
            if !link
                .to_links
                .iter()
                .filter(|link| link.to_entity_id.is_some())
                .filter(|link| {
                    link.to_entity_id.unwrap().0 == entity_id && link.link == Link::SwitchLink
                })
                .collect::<Vec<&ToLink>>()
                .is_empty()
            {
                self.entities_left_to_trigger.insert(producer_entity_id);
            }
        }

        info!(
            "trigger_multi({:?}) - connected to entities: {:?}",
            entity_id, self.entities_left_to_trigger
        );
        Effect::NoEffect
    }
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from } => {
                if !self.entities_left_to_trigger.remove(from) {
                    return Effect::NoEffect;
                }
                // A TriggerMulti is a one-shot "all inputs" latch. Consume the
                // incoming SwitchLink as each source arrives so the remaining
                // input set is part of the ordinary, save-serialized Links
                // graph. Rebuilding this Script after a load can then resume
                // midway through the latch instead of demanding already-used
                // sources again.
                if let Ok(mut links) = world.borrow::<ViewMut<Links>>()
                    && let Ok(source_links) = (&mut links).get(*from)
                {
                    source_links.to_links.retain(|link| {
                        link.link != Link::SwitchLink
                            || link.to_entity_id.is_none_or(|to| to.0 != entity_id)
                    });
                }
                let after_count = self.entities_left_to_trigger.len();
                info!(
                    "turn on from entity {:?}, {} remaining...",
                    from, after_count
                );

                if after_count == 0 {
                    send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    )
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::WrappedEntityId;

    use super::*;

    #[test]
    fn consumed_inputs_survive_script_reinitialization_through_links() {
        let mut world = World::new();
        let destination = world.add_entity(Links::empty());
        let multi = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 3,
                to_entity_id: Some(WrappedEntityId(destination)),
                link: Link::SwitchLink,
            }],
        });
        let first = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 2,
                to_entity_id: Some(WrappedEntityId(multi)),
                link: Link::SwitchLink,
            }],
        });
        let second = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 2,
                to_entity_id: Some(WrappedEntityId(multi)),
                link: Link::SwitchLink,
            }],
        });

        let mut before_save = TriggerMulti::new();
        before_save.initialize(multi, &world);
        let physics = PhysicsWorld::new();
        assert!(matches!(
            before_save.handle_message(
                multi,
                &world,
                &physics,
                &MessagePayload::TurnOn { from: first },
            ),
            Effect::NoEffect
        ));

        let mut after_load = TriggerMulti::new();
        after_load.initialize(multi, &world);
        let effect = after_load.handle_message(
            multi,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: second },
        );
        assert!(matches!(
            effect,
            Effect::Combined { effects }
                if effects.iter().any(|effect| matches!(
                    effect,
                    Effect::Send { msg }
                        if msg.to == destination
                            && matches!(msg.payload, MessagePayload::TurnOn { from } if from == multi)
                ))
        ));
    }
}
