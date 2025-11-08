use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links_and_self};

pub struct TriggerCollide {}

impl TriggerCollide {
    pub fn new() -> TriggerCollide {
        TriggerCollide {}
    }
}
impl Script for TriggerCollide {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from } => send_to_all_switch_links_and_self(
                world,
                entity_id,
                MessagePayload::Collided { with: *from },
            ),
            _ => Effect::NoEffect,
        }
    }
}
