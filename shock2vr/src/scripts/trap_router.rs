use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{script_util::send_to_all_switch_links, Effect, MessagePayload, Script};

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
        send_to_all_switch_links(world, entity_id, msg.clone())
    }
}
