use dark::properties::{PropQuestBitName, PropQuestBitValue, QuestBitValue};
use shipyard::{EntityId, Get, UniqueView, View, World};
use tracing::info;

use crate::{physics::PhysicsWorld, quest_info::QuestInfo, time::Time};

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TrapQuestbitSimple {
    qb_name: String,
    last_value: u32,
}
impl TrapQuestbitSimple {
    pub fn new() -> TrapQuestbitSimple {
        TrapQuestbitSimple {
            qb_name: "".to_owned(),
            last_value: 0,
        }
    }
}
impl Script for TrapQuestbitSimple {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_qbname = world.borrow::<View<PropQuestBitName>>().unwrap();
        let qb_name = &v_qbname.get(entity_id).unwrap().0;
        let current_quest_bit = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let current_value = current_quest_bit.read_quest_bit_value(qb_name).bits();

        self.qb_name = qb_name.to_owned();
        self.last_value = current_value;
        check_value(&self.qb_name, entity_id, world)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        let current_quest_bit = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let current_value = current_quest_bit.read_quest_bit_value(&self.qb_name).bits();

        if current_value != self.last_value {
            self.last_value = current_value;
            check_value(&self.qb_name, entity_id, world)
        } else {
            Effect::NoEffect
        }
    }
}
fn check_value(qb_name: &String, entity_id: EntityId, world: &World) -> Effect {
    let v_qbval = world.borrow::<View<PropQuestBitValue>>().unwrap();

    let check_qbval = v_qbval
        .get(entity_id)
        .map(|v| v.0)
        .unwrap_or(QuestBitValue::UNKNOWN);

    let check_qbval_u32: u32 = check_qbval.bits();

    let current_quest_bit = world.borrow::<UniqueView<QuestInfo>>().unwrap();
    let current_value = current_quest_bit.read_quest_bit_value(qb_name).bits();
    info!(
        "comparing qb val for {} - current: {}, compare: {}",
        qb_name, current_value, check_qbval_u32
    );

    if current_value > check_qbval_u32 {
        info!("-- check passed, sending TurnOn",);
        Effect::Combined {
            effects: vec![
                send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                ),
                Effect::DestroyEntity { entity_id },
            ],
        }
    } else {
        Effect::NoEffect
    }
}
