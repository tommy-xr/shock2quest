use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::set_quest_bit_effect};

pub struct TrapQBSet {}
impl TrapQBSet {
    pub fn new() -> TrapQBSet {
        TrapQBSet {}
    }
}
impl Script for TrapQBSet {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                if let Some(quest_bit_effect) = set_quest_bit_effect(world, entity_id) {
                    Effect::Combined {
                        effects: vec![quest_bit_effect, Effect::DestroyEntity { entity_id }],
                    }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
