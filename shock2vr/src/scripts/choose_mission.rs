use dark::properties::{PropCharGenRo, PropDestLevel, PropDestLoc, QuestBitValue};
use shipyard::{EntityId, Get, UniqueView, View, World};
use tracing::info;

use crate::{career::Career, physics::PhysicsWorld, quest_info::QuestInfo};

use super::{Effect, MessagePayload, Script};

pub struct ChooseMissionScript {}

impl ChooseMissionScript {
    pub fn new() -> ChooseMissionScript {
        ChooseMissionScript {}
    }

    fn get_current_year(world: &World) -> u32 {
        let quest_info = world.borrow::<UniqueView<QuestInfo>>().unwrap();

        // The HIGHEST completed training year (default 1 when none set), so each
        // trigger advances the counter monotonically. Returning the *lowest*
        // completed bit made it stick: once training_year_2 and _3 were both set
        // it always returned 2 -> new_year 3 < 4, re-looping station.mis forever
        // and never reaching the year-4 deploy-to-medsci1 branch.
        let mut highest = 1;
        for year in 1..=4 {
            let quest_bit_name = format!("training_year_{}", year);
            if quest_info.read_quest_bit_value(&quest_bit_name) == QuestBitValue::COMPLETE {
                highest = year;
            }
        }
        highest
    }

    fn set_training_year(year: u32) -> Effect {
        let quest_bit_name = format!("training_year_{}", year);
        Effect::SetQuestBit {
            quest_bit_name,
            quest_bit_value: QuestBitValue::COMPLETE,
        }
    }

    /// The reward effect for completing this tour: the current career (from the
    /// persisted career bit) + the training year being completed (`current_year`,
    /// 1..=3) + the tour index `P$CharGenRo` carried by the tour marker
    /// (`entity_id`). `Effect::NoEffect` when no career is set or the marker
    /// lacks a tour index. The grant is applied at most once per training year
    /// (see `PlayerStats::apply_tour_reward`).
    fn grant_reward_effect(world: &World, entity_id: EntityId, current_year: u32) -> Effect {
        let career = {
            let quest_info = world.borrow::<UniqueView<QuestInfo>>().unwrap();
            Career::from_quest_info(&quest_info)
        };
        let Some(career) = career else {
            return Effect::NoEffect;
        };
        // A malformed (negative) tour index is rejected rather than clamped to
        // tour 0, so bad data grants nothing instead of the wrong reward
        // (`tour_reward` validates the 0..=2 range on the u32).
        let tour = world
            .borrow::<View<PropCharGenRo>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|c| c.0))
            .and_then(|raw| u32::try_from(raw).ok());
        let Some(tour) = tour else {
            return Effect::NoEffect;
        };
        Effect::GrantTourReward {
            career,
            year: current_year,
            tour,
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
                // Get current training year and increment it. `current_year` is
                // the year the player just completed (1..=3): the first tour
                // fires with no `training_year_*` bit set (current 1 -> new 2),
                // the second with `_2` set (current 2 -> new 3), the third with
                // `_2`+`_3` (current 3 -> new 4 = deploy). NB: `training_year_1`
                // is never authored - the counter starts at `_2`.
                let current_year = Self::get_current_year(world);
                let new_year = current_year + 1;

                // Create effect to set the new year quest bit
                let set_year_effect = Self::set_training_year(new_year);

                // Grant this tour's stat/skill reward for (career, completed
                // year, tour index). Applies at most once per training year.
                let grant_effect = Self::grant_reward_effect(world, entity_id, current_year);

                if new_year < 4 {
                    info!(
                        "ChooseMission: Handling year {} (< 4), returning to station.mis",
                        new_year
                    );
                    // For years 1, 2: loop back to station.mis for the next tour.
                    // The transition uses the level's default spawn (loc: None);
                    // the marker's PropDestLoc is only used on the final deploy.

                    Effect::Multiple(vec![
                        set_year_effect,
                        grant_effect,
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
                        grant_effect,
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
