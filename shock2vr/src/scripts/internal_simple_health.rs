use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

// Script to handle simple health behavior
pub struct InternalSimpleHealth {}

impl InternalSimpleHealth {
    pub fn new() -> InternalSimpleHealth {
        InternalSimpleHealth {}
    }
}

impl Script for InternalSimpleHealth {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Damage { .. } => Effect::SlayEntity { entity_id },
            _ => Effect::NoEffect,
        }
    }
}
