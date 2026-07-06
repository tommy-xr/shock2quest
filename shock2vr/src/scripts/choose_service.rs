use dark::properties::{PropService, QuestBitValue};
use shipyard::{EntityId, Get, View, World};

use crate::career::Career;
use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The station recruit-deck script that records which service branch
/// (Marine/Navy/OSA) the player enlisted in and deploys them to MedSci 1.
///
/// The branch comes from the entity's `P$Service` value; it is registered as a
/// persisted career quest bit (so it applies to the player on deployment - see
/// `crate::career`) and the year-1 training quest bit is completed. Attaching
/// this script to the in-world station debrief UI is still deferred (issue
/// #424); the `SelectCareer*` input actions drive the same registration
/// meanwhile.
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
                // Read the enlisted branch from this entity's P$Service value.
                let v_service = world.borrow::<View<PropService>>().unwrap();
                let service = v_service.get(entity_id).map(|s| s.0).unwrap_or(0);
                let career = Career::from_service(service);

                let mut effects = career.select_effects();
                effects.push(Effect::SetQuestBit {
                    quest_bit_name: "training_year_1".to_string(),
                    quest_bit_value: QuestBitValue::COMPLETE,
                });
                // TODO(#424): fully wire the in-world station debrief sequence.
                effects.push(Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    level_file: "medsci1.mis".to_owned(),
                    loc: None,
                    entities_to_trigger: vec!["DEBRIEF1-DOOR".to_string()],
                }));
                effects.push(super::script_util::send_to_all_switch_links_and_self(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                ));

                Effect::Multiple(effects)
            }
            _ => Effect::NoEffect,
        }
    }
}
