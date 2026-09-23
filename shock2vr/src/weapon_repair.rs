//! Repairing a Broken gun on the HRM board. Winning sets it back to Normal
//! with 10 more condition; a critical failure destroys it.
use crate::{player_stats::Skill, quest_info::QuestInfo};
use dark::properties::{
    ObjectState, PropHackDiff, PropObjState, PropRepairDiff, PropRequiredTechDesc,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::scripts::Effect;

/// Condition a won repair adds, capped at 100.
const REPAIR_CONDITION_BONUS: f32 = 10.0;

pub fn is_broken(world: &World, weapon: EntityId) -> bool {
    world
        .borrow::<View<PropObjState>>()
        .is_ok_and(|v| v.get(weapon).is_ok_and(|p| p.0 == ObjectState::Broken))
}

pub fn supported(world: &World, weapon: EntityId) -> bool {
    world
        .borrow::<View<PropRepairDiff>>()
        .is_ok_and(|v| v.contains(weapon))
}

/// The terms of one repair attempt on the wielded `weapon`, or why it cannot
/// be attempted.
pub fn quote(world: &World, weapon: EntityId) -> Result<PropHackDiff, String> {
    if crate::wielded_weapon::resolve_weapon_target(world, Some(weapon)) != Some(weapon) {
        return Err("Wield the weapon to repair it.".into());
    }
    if !is_broken(world, weapon) {
        return Err("The weapon is not broken.".into());
    }
    let mut diff = world
        .borrow::<View<PropRepairDiff>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|p| p.0))
        .ok_or("This weapon cannot be repaired.")?;
    let required = world
        .borrow::<View<PropRequiredTechDesc>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|p| p.0.repair()))
        .unwrap_or(1);
    let quests = world
        .borrow::<UniqueView<QuestInfo>>()
        .map_err(|_| "No player stats.")?;
    if quests.player_stats().skill_level(Skill::Repair) < required {
        return Err(format!("Requires trained Repair {required}."));
    }
    diff.cost = (diff.cost as i32).max(1) as f32;
    Ok(diff)
}

pub fn success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::combine(vec![
        Effect::SetObjectState {
            entity_id,
            state: ObjectState::Normal,
        },
        Effect::AdjustWeaponCondition {
            entity_id,
            delta: REPAIR_CONDITION_BONUS,
        },
    ])
}

pub fn critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::DestroyEntity { entity_id }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::PlayerInfo;
    use dark::properties::{PropGunState, TechSkillValues};

    fn broken_pistol(repair_skill: i32) -> (World, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity((
            PropGunState {
                ammo: 6,
                condition: 0.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropObjState(ObjectState::Broken),
            PropRepairDiff(PropHackDiff {
                success_chance: 20,
                critical_chance: 4,
                cost: 3.0,
            }),
            PropRequiredTechDesc(TechSkillValues([0, 2, 0, 0, 0])),
        ));
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: inventory,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(gun),
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().skills.repair = repair_skill;
        world.add_unique(quests);
        (world, gun)
    }

    #[test]
    fn quote_needs_a_wielded_broken_gun_and_the_authored_repair_skill() {
        let (world, gun) = broken_pistol(1);
        assert_eq!(
            quote(&world, gun).unwrap_err(),
            "Requires trained Repair 2."
        );

        let (mut world, gun) = broken_pistol(2);
        assert_eq!(quote(&world, gun).unwrap().cost, 3.0);

        world.add_component(gun, PropObjState(ObjectState::Normal));
        assert_eq!(quote(&world, gun).unwrap_err(), "The weapon is not broken.");
    }

    #[test]
    fn a_win_restores_the_gun_and_a_critical_failure_destroys_it() {
        let (world, gun) = broken_pistol(2);
        assert!(matches!(
            Effect::flatten(vec![success(gun, &world)]).as_slice(),
            [
                Effect::SetObjectState { entity_id: a, state: ObjectState::Normal },
                Effect::AdjustWeaponCondition { entity_id: b, delta },
            ] if *a == gun && *b == gun && *delta == REPAIR_CONDITION_BONUS
        ));
        assert!(matches!(
            critical_failure(gun, &world),
            Effect::DestroyEntity { entity_id } if entity_id == gun
        ));
    }
}
