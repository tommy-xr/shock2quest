//! `STRBoost` / `INTBoost` (retail `BaseStatBoostScript`): using the hypo
//! adds its `P$DrugStats` bonus for five minutes (six with Pharmo-Friendly)
//! and consumes one from the stack. An unresearched INT Boost does nothing.
use std::time::Duration;

use dark::properties::{ObjectState, PropDrugStats, PropObjState};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::player_stats::Stat;

use super::{Effect, MessagePayload, Script};

const DURATION: Duration = Duration::from_secs(5 * 60);

/// `P$DrugStats` slots, in retail order (STR, END, PSI, AGI, CYB).
const RETAIL_STAT_ORDER: [Stat; 5] = [
    Stat::Strength,
    Stat::Endurance,
    Stat::PsionicAbility,
    Stat::Agility,
    Stat::CyberAffinity,
];

pub struct StatBooster;

impl Script for StatBooster {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }
        let unresearched = world.borrow::<View<PropObjState>>().is_ok_and(|v| {
            v.get(entity_id)
                .is_ok_and(|s| s.0 == ObjectState::Unresearched)
        });
        if unresearched {
            return Effect::NoEffect;
        }
        let Some(stats) = world
            .borrow::<View<PropDrugStats>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|s| s.0))
        else {
            return Effect::NoEffect;
        };
        let boosts = RETAIL_STAT_ORDER
            .into_iter()
            .zip(stats)
            .filter(|(_, delta)| *delta != 0)
            .collect();
        let duration = super::healing_item::HealingItemScript::retail_duration(world, DURATION);
        Effect::UseStatBooster {
            entity_id,
            boosts,
            duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn use_booster(world: &World, booster: EntityId) -> Effect {
        StatBooster.handle_message(booster, world, &PhysicsWorld::new(), &MessagePayload::Frob)
    }

    #[test]
    fn int_boost_requests_one_psi_for_five_minutes() {
        let mut world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        let booster = world.add_entity(PropDrugStats([0, 0, 1, 0, 0]));
        let Effect::UseStatBooster {
            entity_id,
            boosts,
            duration,
        } = use_booster(&world, booster)
        else {
            panic!("expected a booster use");
        };
        assert_eq!(entity_id, booster);
        assert_eq!(boosts, vec![(Stat::PsionicAbility, 1)]);
        assert_eq!(duration, DURATION);
    }

    #[test]
    fn pharmo_friendly_extends_to_six_minutes() {
        let mut world = World::new();
        let mut quests = crate::quest_info::QuestInfo::new();
        assert!(
            quests
                .player_stats_mut()
                .add_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY)
        );
        world.add_unique(quests);
        let booster = world.add_entity(PropDrugStats([1, 0, 0, 0, 0]));
        let Effect::UseStatBooster { duration, .. } = use_booster(&world, booster) else {
            panic!("expected a booster use");
        };
        assert_eq!(duration, Duration::from_secs(6 * 60));
    }

    #[test]
    fn unresearched_booster_does_nothing() {
        let mut world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        let booster = world.add_entity((
            PropDrugStats([0, 0, 1, 0, 0]),
            PropObjState(ObjectState::Unresearched),
        ));
        assert!(matches!(use_booster(&world, booster), Effect::NoEffect));
    }
}
