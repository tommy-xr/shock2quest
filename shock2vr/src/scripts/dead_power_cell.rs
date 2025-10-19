use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct DeadPowerCell;
impl DeadPowerCell {
    pub fn new() -> DeadPowerCell {
        DeadPowerCell
    }
}

impl Script for DeadPowerCell {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Recharge => Effect::ReplaceEntity {
                entity_id,
                template_id: -1863, /* charged power cell */
            },
            _ => Effect::NoEffect,
        }
    }
}
