use cgmath::Vector3;
use dark::properties::{
    Link, PropDestLevel, PropDestLoc, PropPosition, PropStartLoc, QuestBitValue,
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    physics::PhysicsWorld,
    quest_info::QuestInfo,
    scripts::script_util::{get_all_links_of_type, get_first_link_of_type},
};

use super::{Effect, MessagePayload, Script};

pub struct ChooseMissionScript {}

impl ChooseMissionScript {
    pub fn new() -> ChooseMissionScript {
        ChooseMissionScript {}
    }

    fn get_current_year(world: &World) -> u32 {
        let quest_info = world.borrow::<UniqueView<QuestInfo>>().unwrap();

        // Check for training year quest bits (training_year_1, training_year_2, etc.)
        for year in 1..=4 {
            let quest_bit_name = format!("training_year_{}", year);
            let quest_bit_value = quest_info.read_quest_bit_value(&quest_bit_name);
            if quest_bit_value == QuestBitValue::COMPLETE {
                return year;
            }
        }

        // If no training year quest bits are set, default to year 1
        1
    }

    fn set_training_year(year: u32) -> Effect {
        let quest_bit_name = format!("training_year_{}", year);
        Effect::SetQuestBit {
            quest_bit_name,
            quest_bit_value: QuestBitValue::COMPLETE,
        }
    }
}

impl Script for ChooseMissionScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                // Get current training year and increment it
                let current_year = Self::get_current_year(world);
                let new_year = current_year + 1;

                // Create effect to set the new year quest bit
                let set_year_effect = Self::set_training_year(new_year);

                if new_year < 4 {
                    println!("-- Handling year < 4 chase");
                    // For years 1, 2, 3: only teleport to PropDestLoc (stay in current mission)

                    Effect::Multiple(vec![
                        set_year_effect,
                        Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                            level_file: "station.mis".to_string(),
                            loc: None,
                            entities_to_trigger: vec![],
                        }),
                    ])
                } else {
                    // For year 4: go to PropDestLevel (final destination)
                    let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
                    let level_file = v_dest_level
                        .get(entity_id)
                        .map(|level| format!("{}.mis", level.0))
                        .unwrap_or_else(|_| "medsci1.mis".to_string());

                    let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
                    let dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);

                    Effect::Multiple(vec![
                        set_year_effect,
                        Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                            level_file,
                            loc: dest_loc,
                            entities_to_trigger: vec![],
                        }),
                    ])
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
