use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

pub struct ToolConsumable;
impl ToolConsumable {
    pub fn new() -> ToolConsumable {
        ToolConsumable
    }
}

impl Script for ToolConsumable {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with } => Effect::Send {
                msg: Message {
                    to: *with,
                    payload: MessagePayload::ProvideForConsumption { entity: entity_id },
                },
            },
            _ => Effect::NoEffect,
        }
    }
}
