use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
    script_util::{invert, send_to_all_switch_links},
};

pub struct TrapInverter {}
impl TrapInverter {
    pub fn new() -> TrapInverter {
        TrapInverter {}
    }
}
impl Script for TrapInverter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let inverted_message = invert(msg.clone());
        send_to_all_switch_links(world, entity_id, inverted_message)
    }
}
