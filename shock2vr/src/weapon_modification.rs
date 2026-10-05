//! Ranged-family identity and authored Modify skill, difficulty and pricing.
//! Stats and installations are evaluated by the selectable upgrade system.
use crate::{player_stats::Skill, quest_info::QuestInfo};
use dark::properties::{
    ObjectState, PropGunState, PropHackDiff, PropModify2Diff, PropModifyDiff, PropObjState,
    PropRequiredTechDesc, PropScripts,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Pistol,
    Shotgun,
    Rifle,
    Laser,
    Emp,
    Fusion,
    Stasis,
    Grenade,
    Annelid,
    Viral,
}
fn kind(world: &World, weapon: EntityId) -> Option<Kind> {
    world
        .borrow::<View<PropScripts>>()
        .ok()?
        .get(weapon)
        .ok()?
        .scripts
        .iter()
        .find_map(|script| {
            Some(match script.to_ascii_lowercase().as_str() {
                "pistolmodify" => Kind::Pistol,
                "shotgunmodify" => Kind::Shotgun,
                "riflemodify" => Kind::Rifle,
                "lasermodify" => Kind::Laser,
                "empmodify" => Kind::Emp,
                "fusionmodify" => Kind::Fusion,
                "stasismodify" => Kind::Stasis,
                "grenademodify" => Kind::Grenade,
                "annelidmodify" => Kind::Annelid,
                "viralmodify" => Kind::Viral,
                _ => return None,
            })
        })
}
pub fn level(world: &World, weapon: EntityId) -> Option<i32> {
    if let Ok(upgrades) = world.borrow::<View<crate::weapon_upgrades::WeaponUpgrades>>() {
        if let Ok(upgrades) = upgrades.get(weapon) {
            return Some(upgrades.tier() as i32);
        }
    }
    world
        .borrow::<View<PropGunState>>()
        .ok()?
        .get(weapon)
        .ok()
        .map(|p| p.modification)
}
pub fn supported(world: &World, weapon: EntityId) -> bool {
    kind(world, weapon).is_some()
}

/// Biological weapons grow both fire modes; neither costs an upgrade slot.
pub(crate) fn innate_alternate_fire(world: &World, weapon: EntityId) -> bool {
    matches!(kind(world, weapon), Some(Kind::Annelid | Kind::Viral))
}

/// Stasis stimulus controls a non-damaging effect and must not receive the
/// generic damage bonus. Its capacity, wear and alternate mode still upgrade.
pub(crate) fn scales_damage(world: &World, weapon: EntityId) -> bool {
    kind(world, weapon) != Some(Kind::Stasis)
}

/// Authored skill, target and HRM pricing rules for selectable installations.
pub(crate) fn paid_quote(world: &World, weapon: EntityId) -> Result<PropHackDiff, String> {
    if crate::wielded_weapon::resolve_weapon_target(world, Some(weapon)) != Some(weapon)
        && !crate::scripts::gui::WeaponSettingsTarget::permits_device_job(world, weapon)
    {
        return Err("Wield the weapon to modify it.".into());
    }
    if !supported(world, weapon) {
        return Err("This weapon cannot be modified.".into());
    }
    if world
        .borrow::<View<PropObjState>>()
        .is_ok_and(|v| v.get(weapon).is_ok_and(|p| p.0 != ObjectState::Normal))
    {
        return Err("The weapon must be functional and researched.".into());
    }
    let level = level(world, weapon).ok_or("No gun state.")?;
    if !(0..2).contains(&level) {
        return Err(crate::scripts::gui::PanelText::hrm(
            world,
            "ModifyResult3",
            "This weapon cannot be modified any further.",
            &[],
        ));
    }
    let required = world
        .borrow::<View<PropRequiredTechDesc>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|p| p.0.0[2]))
        .unwrap_or(1)
        + 2 * level;
    let quests = world
        .borrow::<UniqueView<QuestInfo>>()
        .map_err(|_| "No player stats.")?;
    if quests.player_stats().skill_level(Skill::Modify) < required {
        return Err(crate::scripts::gui::PanelText::hrm(
            world,
            "techminskill2",
            "Modify skill %d required.",
            &[required],
        ));
    }
    let diff = if level == 0 {
        world
            .borrow::<View<PropModifyDiff>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|p| p.0))
    } else {
        world
            .borrow::<View<PropModify2Diff>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|p| p.0))
    };
    let mut diff = diff.ok_or("No modification difficulty authored.")?;
    // shkhrm.cpp FindCost: divide before integer truncation, minimum one.
    if quests
        .player_stats()
        .has_os_trait(crate::scripts::gui::TRAIT_TINKER)
    {
        diff.cost /= 2.0;
    }
    diff.cost = (diff.cost as i32).max(1) as f32;
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{GunSettingDesc, PropBaseGunDesc};
    #[test]
    fn tinker_discounts_both_levels_and_never_bypasses_training_or_sequence() {
        use dark::properties::{PropRequiredTechDesc, TechSkillValues};
        let mut world = World::new();
        let gun = world.add_entity((
            PropScripts {
                scripts: vec!["PistolModify".into()],
                inherits: true,
            },
            PropGunState {
                ammo: 6,
                condition: 100.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropBaseGunDesc {
                settings: std::array::from_fn(|_| GunSettingDesc {
                    clip: 12,
                    ..Default::default()
                }),
            },
            PropModifyDiff(PropHackDiff {
                success_chance: 20,
                critical_chance: 2,
                cost: 21.0,
            }),
            PropModify2Diff(PropHackDiff {
                success_chance: 10,
                critical_chance: 3,
                cost: 1.0,
            }),
            PropRequiredTechDesc(TechSkillValues([0, 0, 2, 0, 0])),
        ));
        world.add_unique(crate::mission::PlayerInfo {
            entity_id: gun,
            inventory_entity_id: gun,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(gun),
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().skills.modify = 2;
        world.add_unique(quests);
        assert_eq!(paid_quote(&world, gun).unwrap().cost, 21.0);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .add_os_trait(15);
        assert_eq!(paid_quote(&world, gun).unwrap().cost, 10.0);
        crate::weapon_installation::install(
            &mut world,
            gun,
            crate::weapon_upgrades::WeaponUpgrade::ExtendedCapacity,
            0,
            crate::weapon_installation::Payment::Modify,
        );
        assert_eq!(level(&world, gun), Some(1));
        assert!(
            paid_quote(&world, gun)
                .unwrap_err()
                .contains("Modify skill 4")
        );
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 4;
        assert_eq!(paid_quote(&world, gun).unwrap().cost, 1.0);
        crate::weapon_installation::install(
            &mut world,
            gun,
            crate::weapon_upgrades::WeaponUpgrade::LowMaintenanceI,
            1,
            crate::weapon_installation::Payment::Modify,
        );
        assert_eq!(level(&world, gun), Some(2));
        assert!(
            paid_quote(&world, gun)
                .unwrap_err()
                .contains("modified any further")
        );
        world
            .borrow::<shipyard::UniqueViewMut<crate::mission::PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        assert!(paid_quote(&world, gun).is_err());
    }
}
