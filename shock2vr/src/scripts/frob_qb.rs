use dark::properties::{PropQuestBitName, PropQuestBitValue, QuestBitValue};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

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
            MessagePayload::Frob => {
                let v_qbname = world.borrow::<View<PropQuestBitName>>().unwrap();
                let v_qbval = world.borrow::<View<PropQuestBitValue>>().unwrap();

                let check_qbval = v_qbval
                    .get(entity_id)
                    .map(|v| v.0)
                    .unwrap_or(QuestBitValue::COMPLETE);

                if let Ok(qb_name) = &v_qbname.get(entity_id) {
                    Effect::Combined {
                        effects: vec![
                            Effect::SetQuestBit {
                                quest_bit_name: qb_name.0.to_owned(),
                                quest_bit_value: check_qbval,
                            },
                            Effect::DestroyEntity { entity_id },
                        ],
                    }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
