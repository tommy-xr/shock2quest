use dark::properties::{PropQuestBitName, PropQuestBitValue, QuestBitValue};
use shipyard::{EntityId, Get, UniqueView, View, World};
use tracing::info;

use crate::{physics::PhysicsWorld, quest_info::QuestInfo};

use super::{
    Effect, MessagePayload, Script,
    script_util::{is_message_turnon_or_turnoff, send_to_all_switch_links},
};

pub struct TrapQBNegFilter {}
impl TrapQBNegFilter {
    pub fn new() -> TrapQBNegFilter {
        TrapQBNegFilter {}
    }
}
impl Script for TrapQBNegFilter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if is_message_turnon_or_turnoff(msg) {
            // Check quest bit
            let v_qbname = world.borrow::<View<PropQuestBitName>>().unwrap();
            let v_qbval = world.borrow::<View<PropQuestBitValue>>().unwrap();

            let check_qbval = v_qbval
                .get(entity_id)
                .map(|v| v.0)
                .unwrap_or(QuestBitValue::UNKNOWN);

            let check_qbval_u32: u32 = check_qbval.bits();

            let current_quest_bit = world.borrow::<UniqueView<QuestInfo>>().unwrap();

            let qb_name = &v_qbname.get(entity_id).unwrap().0;
            let current_value = current_quest_bit.read_quest_bit_value(qb_name).bits();

            info!(
                "comparing qb val for {} - current: {}, compare: {}",
                qb_name, current_value, check_qbval_u32
            );

            if current_value <= check_qbval_u32 {
                send_to_all_switch_links(world, entity_id, msg.clone())
            } else {
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        }
    }
}
