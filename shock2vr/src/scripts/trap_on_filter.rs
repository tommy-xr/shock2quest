use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TrapOffFilter {}
impl TrapOffFilter {
    pub fn new() -> TrapOffFilter {
        TrapOffFilter {}
    }
}
impl Script for TrapOffFilter {
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
