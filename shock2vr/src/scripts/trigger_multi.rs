use std::collections::HashSet;

use dark::properties::{Link, Links, ToLink};
use shipyard::{EntityId, IntoIter, IntoWithId, View, World};
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
                self.entities_left_to_trigger.remove(from);
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
