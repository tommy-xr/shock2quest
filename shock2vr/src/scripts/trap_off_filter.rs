use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{script_util::send_to_all_switch_links, Effect, MessagePayload, Script};

#[allow(dead_code)]
pub struct TrapOnFilter {}
impl TrapOnFilter {}
impl Script for TrapOnFilter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOff { from: _ } => {
                send_to_all_switch_links(world, entity_id, msg.clone())
            }
            _ => Effect::NoEffect,
        }
    }
}
