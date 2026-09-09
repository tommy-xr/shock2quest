//! Shared authored weapon-skill eligibility and the short-lived refusal shown
//! at the firing hand. The renderer never decides whether a weapon can fire.

use std::time::Duration;

use dark::properties::PropBaseWeaponDesc;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{player_stats::Skill, scripts::script_util::player_skill_level};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponSkillRequirement {
    pub skill: Skill,
    pub required: i32,
    pub current: i32,
}

impl WeaponSkillRequirement {
    pub fn message(self) -> String {
        let name = match self.skill {
            Skill::StandardWeapons => "Standard Weapons",
            Skill::EnergyWeapons => "Energy Weapons",
            Skill::HeavyWeapons => "Heavy Weapons",
            Skill::ExoticWeapons => "Exotic Weapons",
            _ => unreachable!("only weapon skills have weapon requirements"),
        };
        format!(
            "Requires {name} {} - You have {}",
            self.required, self.current
        )
    }
}

/// First unmet requirement, in the same class order as Dark's CheckWeaponSkills.
/// Unauthored requirements impose no gate (e.g. the psi amp's separate path).
pub fn unmet_weapon_skill(world: &World, weapon: EntityId) -> Option<WeaponSkillRequirement> {
    let requirements = world.borrow::<View<PropBaseWeaponDesc>>().ok()?;
    let requirements = requirements.get(weapon).ok()?;
    [
        Skill::StandardWeapons,
        Skill::EnergyWeapons,
        Skill::HeavyWeapons,
        Skill::ExoticWeapons,
    ]
    .into_iter()
    .zip(requirements.0)
    .find_map(|(skill, required)| {
        let current = player_skill_level(world, skill);
        (current < required).then_some(WeaponSkillRequirement {
            skill,
            required,
            current,
        })
    })
}

/// Transient visual state on the rejected weapon; never serialized. Binding it
/// to the weapon keeps one hand's refusal off the other hand's gun.
#[derive(Clone, Debug, Component)]
pub struct WeaponSkillNotice {
    pub requirement: WeaponSkillRequirement,
    pub expires: Duration,
}

pub fn active_weapon_skill_notice(
    world: &World,
    weapon: EntityId,
) -> Option<WeaponSkillRequirement> {
    let notices = world.borrow::<View<WeaponSkillNotice>>().ok()?;
    let notice = notices.get(weapon).ok()?;
    let now = world
        .borrow::<shipyard::UniqueView<crate::time::Time>>()
        .ok()?
        .total;
    (notice.expires > now && unmet_weapon_skill(world, weapon) == Some(notice.requirement))
        .then_some(notice.requirement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;

    #[test]
    fn required_skill_threshold_is_inclusive_and_classes_are_independent() {
        let mut world = World::new();
        let weapon = world.add_entity(PropBaseWeaponDesc([6, 0, 0, 0]));
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().skills.standard_weapons = 4;
        quests.player_stats_mut().skills.energy_weapons = 6;
        world.add_unique(quests);
        let failure = unmet_weapon_skill(&world, weapon).unwrap();
        assert_eq!(
            failure.message(),
            "Requires Standard Weapons 6 - You have 4"
        );
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .standard_weapons = 6;
        assert!(unmet_weapon_skill(&world, weapon).is_none());
    }

    #[test]
    fn no_authored_requirement_allows_use() {
        let mut world = World::new();
        let weapon = world.add_entity(());
        assert!(unmet_weapon_skill(&world, weapon).is_none());
    }

    #[test]
    fn factory_skills_allow_training_without_upgrading_the_persistent_sheet() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let player = world.add_entity(PropBaseWeaponDesc([1, 2, 0, 0]));
        world.add_unique(crate::mission::PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: player,
        });
        let pistol = world.add_entity(PropBaseWeaponDesc([1, 0, 0, 0]));
        let laser = world.add_entity(PropBaseWeaponDesc([0, 2, 0, 0]));
        assert!(unmet_weapon_skill(&world, pistol).is_none());
        assert!(unmet_weapon_skill(&world, laser).is_none());
        assert_eq!(
            world
                .borrow::<shipyard::UniqueView<QuestInfo>>()
                .unwrap()
                .player_stats()
                .skills
                .energy_weapons,
            0
        );
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .standard_weapons = 4;
        assert_eq!(player_skill_level(&world, Skill::StandardWeapons), 4);
        // A new mission rederives the player component instead of carrying
        // training allowances into the recruit's persistent career.
        world.remove::<(PropBaseWeaponDesc,)>(player);
        assert_eq!(player_skill_level(&world, Skill::EnergyWeapons), 0);
    }

    #[test]
    fn notices_expire_and_disappear_when_the_requirement_is_met() {
        let mut world = World::new();
        world.add_unique(crate::time::Time::default());
        world.add_unique(QuestInfo::new());
        let weapon = world.add_entity(PropBaseWeaponDesc([6, 0, 0, 0]));
        let other_weapon = world.add_entity(PropBaseWeaponDesc([6, 0, 0, 0]));
        let requirement = unmet_weapon_skill(&world, weapon).unwrap();
        world.add_component(
            weapon,
            WeaponSkillNotice {
                requirement,
                expires: Duration::from_secs(3),
            },
        );
        assert_eq!(
            active_weapon_skill_notice(&world, weapon),
            Some(requirement)
        );
        assert!(active_weapon_skill_notice(&world, other_weapon).is_none());
        world
            .borrow::<shipyard::UniqueViewMut<crate::time::Time>>()
            .unwrap()
            .total = Duration::from_secs(3);
        assert!(active_weapon_skill_notice(&world, weapon).is_none());
        world
            .borrow::<shipyard::UniqueViewMut<crate::time::Time>>()
            .unwrap()
            .total = Duration::ZERO;
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .standard_weapons = 6;
        assert!(active_weapon_skill_notice(&world, weapon).is_none());
    }
}
