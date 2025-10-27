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

    fn get_position_from_loc(world: &World, loc: i32) -> Option<Vector3<f32>> {
        let mut result_position = None;

        world.run(
            |v_position: View<PropPosition>, v_start_loc: View<PropStartLoc>| {
                let mut spawn_entity_id = None;
                let mut closest_delta = u32::MAX;

                for (entity_id, start_loc) in (&v_start_loc).iter().with_id() {
                    // Find the location that matches the best
                    let diff = start_loc.0.abs_diff(loc);

                    // Only consider points that actually are linked to landing points
                    let all_links = get_all_links_of_type(world, entity_id, Link::LandingPoint);

                    if diff < closest_delta && !all_links.is_empty() {
                        closest_delta = diff;
                        spawn_entity_id =
                            get_first_link_of_type(world, entity_id, Link::LandingPoint);
                    }
                }

                if let Some(entity_id) = spawn_entity_id {
                    if let Ok(spawn_pos) = v_position.get(entity_id) {
                        result_position = Some(spawn_pos.position);
                    }
                }
            },
        );

        result_position
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
                    let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
                    let dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);

                    if let Some(loc) = dest_loc {
                        println!("Got the loc prop: {}", loc);
                        if let Some(position) = Self::get_position_from_loc(world, loc) {
                            println!("-- Found loc!");
                            Effect::Multiple(vec![
                                set_year_effect,
                                Effect::SetPlayerPosition {
                                    position,
                                    is_teleport: true,
                                },
                            ])
                        } else {
                            // If we can't find the position, just set the year
                            println!("No loc found...");
                            set_year_effect
                        }
                    } else {
                        // If no destination location, just set the year
                        println!("No PropDestLoc on entity");
                        set_year_effect
                    }
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
