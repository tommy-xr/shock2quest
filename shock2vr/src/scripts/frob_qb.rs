use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::set_quest_bit_effect};

pub struct FrobQB {}
impl FrobQB {
    pub fn new() -> FrobQB {
        FrobQB {}
    }
}
impl Script for FrobQB {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => match set_quest_bit_effect(world, entity_id) {
                Some(quest_bit_effect) => Effect::Combined {
                    effects: vec![quest_bit_effect, Effect::DestroyEntity { entity_id }],
                },
                None => Effect::NoEffect,
            },
            _ => Effect::NoEffect,
        }
    }
}
