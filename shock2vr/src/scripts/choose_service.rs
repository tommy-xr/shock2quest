use dark::properties::{PropService, QuestBitValue};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct ChooseServiceScript {}
impl ChooseServiceScript {
    pub fn new() -> ChooseServiceScript {
        ChooseServiceScript {}
    }
}

impl Script for ChooseServiceScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                // Get the service type from the P$Service property
                let v_service = world.borrow::<View<PropService>>().unwrap();
                let service = v_service.get(entity_id).map(|s| s.0).unwrap_or(0);

                // Determine which entities to trigger based on service type
                let mut entities_to_trigger = vec!["DEBRIEF1-DOOR".to_string()]; // Always trigger DEBRIEF1-DOOR

                let start_entity = match service {
                    0 => "START_01".to_string(), // Marines
                    1 => "START_11".to_string(), // Navy
                    2 => "START_21".to_string(), // OSA
                    _ => "START_01".to_string(), // Unknown service type, default to Marines
                };
                entities_to_trigger.push(start_entity);

                Effect::Multiple(vec![
                    // Updat quest bit
                    Effect::SetQuestBit {
                        quest_bit_name: "training_year_1".to_string(),
                        quest_bit_value: QuestBitValue::COMPLETE,
                    },
                    // TODO: Fully implement station.msi
                    Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                        level_file: "medsci1.mis".to_owned(),
                        loc: None,
                        entities_to_trigger,
                    }),
                    // Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    //     level_file: "station.mis".to_owned(),
                    //     loc: None,
                    //     entities_to_trigger,
                    // }),
                    // Also trigger switch links in the current mission
                    super::script_util::send_to_all_switch_links_and_self(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    ),
                ])
            }
            _ => Effect::NoEffect,
        }
    }
}
