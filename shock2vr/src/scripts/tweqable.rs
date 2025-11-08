use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::change_to_last_model};

pub struct Tweqable {}
impl Tweqable {
    pub fn new() -> Tweqable {
        Tweqable {}
    }
}
impl Script for Tweqable {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => change_to_last_model(world, entity_id),
            _ => Effect::NoEffect,
        }
    }
}
