use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct TrapTweq {}
impl TrapTweq {
    pub fn new() -> TrapTweq {
        TrapTweq {}
    }
}
impl Script for TrapTweq {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => Effect::TurnOnTweqs { entity_id },
            MessagePayload::TurnOff { from: _ } => Effect::TurnOffTweqs { entity_id },
            _ => Effect::NoEffect,
        }
    }
}
