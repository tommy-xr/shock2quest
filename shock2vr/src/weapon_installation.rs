//! Selectable installation policy and its single commit point. Initially the
//! pistol uses this flow; other families retain their retail path until audited.
use dark::properties::{ObjectState, PropGunState};
use shipyard::{EntityId, Get, View, World};

use crate::{
    scripts::Effect,
    weapon_upgrades::{UpgradeSource, WeaponUpgrade, WeaponUpgrades},
};

pub fn supported(world: &World, weapon: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, weapon, "PistolModify")
}

pub fn state(world: &World, weapon: EntityId) -> WeaponUpgrades {
    world
        .borrow::<View<WeaponUpgrades>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().cloned())
        .unwrap_or_default()
}

pub const AVAILABLE: [WeaponUpgrade; 3] = [
    WeaponUpgrade::LowMaintenanceI,
    WeaponUpgrade::LowMaintenanceII,
    WeaponUpgrade::ExtendedCapacity,
];

pub fn label(choice: WeaponUpgrade) -> &'static str {
    match choice {
        WeaponUpgrade::Flashlight => "Flashlight",
        WeaponUpgrade::Laser => "Laser pointer",
        WeaponUpgrade::Silencer => "Silencer",
        WeaponUpgrade::LowMaintenanceI => "Low Maintenance I",
        WeaponUpgrade::LowMaintenanceII => "Low Maintenance II",
        WeaponUpgrade::AlternateFire => "Alternate fire",
        WeaponUpgrade::ExtendedCapacity => "Double capacity",
    }
}

pub fn validate(
    world: &World,
    weapon: EntityId,
    choice: WeaponUpgrade,
    tier: usize,
) -> Result<WeaponUpgrades, String> {
    if !supported(world, weapon) {
        return Err("Selectable upgrades are not available for this weapon yet.".into());
    }
    let gun = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().cloned())
        .ok_or("No gun state.")?;
    let upgrades = state(world, weapon);
    if gun.modification > 0 && upgrades.tier() == 0 {
        return Err("This weapon already has legacy modifications.".into());
    }
    if crate::scripts::gui::object_state(world, weapon) != ObjectState::Normal {
        return Err("The weapon must be functional and researched.".into());
    }
    if crate::wielded_weapon::resolve_weapon_target(world, Some(weapon)) != Some(weapon)
        && !crate::scripts::gui::WeaponSettingsTarget::permits_device_job(world, weapon)
    {
        return Err("Wield the weapon to modify it.".into());
    }
    if !AVAILABLE.contains(&choice) {
        return Err("This upgrade is not available yet.".into());
    }
    upgrades
        .with_upgrade(choice, &AVAILABLE, UpgradeSource::Modify, tier)
        .map_err(|e| e.to_string())
}

pub fn quote(
    world: &World,
    weapon: EntityId,
    choice: WeaponUpgrade,
    tier: usize,
) -> Result<dark::properties::PropHackDiff, String> {
    validate(world, weapon, choice, tier)?;
    crate::weapon_modification::paid_quote(world, weapon)
}

pub fn preview(world: &World, weapon: EntityId, choice: WeaponUpgrade, tier: usize) -> String {
    let next = match validate(world, weapon, choice, tier) {
        Ok(next) => next,
        Err(reason) => return reason,
    };
    let current = state(world, weapon);
    let capacity = if choice == WeaponUpgrade::ExtendedCapacity {
        " Capacity doubles; no free ammo."
    } else {
        ""
    };
    format!(
        "Damage {:.0}% -> {:.0}%. Wear {:.0}% -> {:.0}%.{capacity}",
        current.damage_multiplier() * 100.0,
        next.damage_multiplier() * 100.0,
        current.wear_multiplier() * 100.0,
        next.wear_multiplier() * 100.0
    )
}

/// Called only by the mission effect applier after a paid board wins. The
/// expected tier makes repeated or stale wins inert; no base property mutates.
pub fn install(world: &mut World, weapon: EntityId, choice: WeaponUpgrade, tier: usize) -> Effect {
    let result =
        quote(world, weapon, choice, tier).and_then(|_| validate(world, weapon, choice, tier));
    match result {
        Ok(next) => {
            world.add_component(weapon, next);
            Effect::ShowMessage {
                text: format!("Installed {}.", label(choice)),
            }
        }
        Err(text) => Effect::ShowMessage { text },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mission::PlayerInfo, quest_info::QuestInfo};
    use dark::properties::{
        GunSettingDesc, PropBaseGunDesc, PropHackDiff, PropModify2Diff, PropModifyDiff, PropScripts,
    };

    fn fixture() -> (World, EntityId) {
        let mut world = World::new();
        let diff = PropHackDiff {
            cost: 10.0,
            success_chance: 50,
            critical_chance: 2,
        };
        let weapon = world.add_entity((
            PropScripts {
                scripts: vec!["PistolModify".into()],
                inherits: false,
            },
            PropGunState {
                ammo: 7,
                condition: 81.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropModifyDiff(diff),
            PropModify2Diff(diff),
            PropBaseGunDesc {
                settings: std::array::from_fn(|_| GunSettingDesc {
                    clip: 12,
                    ..Default::default()
                }),
            },
        ));
        world.add_unique(PlayerInfo {
            entity_id: weapon,
            inventory_entity_id: weapon,
            right_hand_entity_id: Some(weapon),
            left_hand_entity_id: None,
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let mut quests = QuestInfo::new();
        quests.player_stats_mut().skills.modify = 6;
        world.add_unique(quests);
        (world, weapon)
    }

    #[test]
    fn installation_preserves_base_ammo_condition_and_rejects_repeated_or_excess_paid_wins() {
        let (mut world, gun) = fixture();
        let base = world
            .borrow::<View<PropBaseGunDesc>>()
            .unwrap()
            .get(gun)
            .unwrap()
            .clone();
        install(&mut world, gun, WeaponUpgrade::ExtendedCapacity, 0);
        install(&mut world, gun, WeaponUpgrade::LowMaintenanceI, 0);
        assert_eq!(
            state(&world, gun).choices(),
            &[WeaponUpgrade::ExtendedCapacity]
        );
        install(&mut world, gun, WeaponUpgrade::LowMaintenanceI, 1);
        assert_eq!(state(&world, gun).tier(), 2);
        assert!(
            quote(&world, gun, WeaponUpgrade::LowMaintenanceII, 2)
                .unwrap_err()
                .contains("device")
        );
        install(&mut world, gun, WeaponUpgrade::LowMaintenanceII, 2);
        assert_eq!(state(&world, gun).tier(), 2);
        let guns = world.borrow::<View<PropGunState>>().unwrap();
        assert_eq!(guns.get(gun).unwrap().ammo, 7);
        assert_eq!(guns.get(gun).unwrap().condition, 81.0);
        assert_eq!(
            world
                .borrow::<View<PropBaseGunDesc>>()
                .unwrap()
                .get(gun)
                .unwrap()
                .settings,
            base.settings
        );
    }

    #[test]
    fn dropped_broken_untrained_and_unavailable_targets_cannot_install() {
        let (mut world, gun) = fixture();
        assert!(quote(&world, gun, WeaponUpgrade::Laser, 0).is_err());
        assert!(quote(&world, gun, WeaponUpgrade::LowMaintenanceII, 0).is_err());
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 0;
        install(&mut world, gun, WeaponUpgrade::ExtendedCapacity, 0);
        assert_eq!(state(&world, gun).tier(), 0);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 6;
        world.add_component(gun, dark::properties::PropObjState(ObjectState::Broken));
        install(&mut world, gun, WeaponUpgrade::ExtendedCapacity, 0);
        assert_eq!(state(&world, gun).tier(), 0);
        world.add_component(gun, dark::properties::PropObjState(ObjectState::Normal));
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        install(&mut world, gun, WeaponUpgrade::ExtendedCapacity, 0);
        assert_eq!(state(&world, gun).tier(), 0);
    }
}
