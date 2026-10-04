//! Selectable installation policy and its single commit point. Initially the
//! pistol and AR15 use this flow; other families retain their retail path until audited.
use dark::properties::{ObjectState, PropGunState};
use shipyard::{EntityId, Get, View, World};

use crate::{
    scripts::Effect,
    weapon_upgrades::{UpgradeSource, WeaponUpgrade, WeaponUpgrades},
};

pub fn supported(world: &World, weapon: EntityId) -> bool {
    silencer_compatible(world, weapon)
}

pub fn silencer_compatible(world: &World, weapon: EntityId) -> bool {
    ["PistolModify", "RifleModify"]
        .into_iter()
        .any(|script| crate::scripts::script_util::entity_has_script(world, weapon, script))
}

pub fn state(world: &World, weapon: EntityId) -> WeaponUpgrades {
    world
        .borrow::<View<WeaponUpgrades>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().cloned())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payment {
    Modify,
    Device(EntityId),
}

pub fn is_device(world: &World, device: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, device, "FreeModify")
}

pub fn available_device(world: &World) -> Option<EntityId> {
    crate::scripts::script_util::player_carried_items(world)
        .into_iter()
        .find(|id| device_available(world, *id))
}

fn device_available(world: &World, device: EntityId) -> bool {
    is_device(world, device)
        && crate::scripts::script_util::player_carried_items(world).contains(&device)
        && world
            .borrow::<View<dark::properties::PropStackCount>>()
            .ok()
            .and_then(|v| v.get(device).ok().map(|s| s.0))
            .unwrap_or(1)
            > 0
}

/// The purchase route and the lock ship together for this weapon family.
pub fn alternate_unlocked(world: &World, weapon: EntityId) -> bool {
    !supported(world, weapon) || state(world, weapon).has(WeaponUpgrade::AlternateFire)
}

pub const AVAILABLE: [WeaponUpgrade; 7] = [
    WeaponUpgrade::Flashlight,
    WeaponUpgrade::Laser,
    WeaponUpgrade::Silencer,
    WeaponUpgrade::LowMaintenanceI,
    WeaponUpgrade::LowMaintenanceII,
    WeaponUpgrade::ExtendedCapacity,
    WeaponUpgrade::AlternateFire,
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
    payment: Payment,
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
    let source = match payment {
        Payment::Modify => UpgradeSource::Modify,
        Payment::Device(device) => {
            if !device_available(world, device) {
                return Err("The French-Epstein device is no longer carried.".into());
            }
            UpgradeSource::Device
        }
    };
    upgrades
        .with_upgrade(choice, &AVAILABLE, source, tier)
        .map_err(|e| e.to_string())
}

pub fn quote(
    world: &World,
    weapon: EntityId,
    choice: WeaponUpgrade,
    tier: usize,
) -> Result<dark::properties::PropHackDiff, String> {
    validate(world, weapon, choice, tier, Payment::Modify)?;
    crate::weapon_modification::paid_quote(world, weapon)
}

pub fn preview(
    world: &World,
    weapon: EntityId,
    choice: WeaponUpgrade,
    tier: usize,
    payment: Payment,
) -> String {
    let next = match validate(world, weapon, choice, tier, payment) {
        Ok(next) => next,
        Err(reason) => return reason,
    };
    let current = state(world, weapon);
    let capacity = if choice == WeaponUpgrade::ExtendedCapacity {
        " Double capacity; no free ammo."
    } else if choice == WeaponUpgrade::AlternateFire {
        " Unlock alternate fire."
    } else if choice == WeaponUpgrade::Silencer {
        " Quieter shots; suppress muzzle flash."
    } else if choice == WeaponUpgrade::Laser {
        " Laser sight; toggle in Settings."
    } else if choice == WeaponUpgrade::Flashlight {
        " Weapon light; toggle in Settings."
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

/// Called only by the mission effect applier after a paid win or device confirmation. The
/// expected tier makes repeated or stale wins inert; no base property mutates.
pub fn install(
    world: &mut World,
    weapon: EntityId,
    choice: WeaponUpgrade,
    tier: usize,
    payment: Payment,
) -> Effect {
    let result = validate(world, weapon, choice, tier, payment).and_then(|next| {
        if payment == Payment::Modify {
            quote(world, weapon, choice, tier)?;
        }
        Ok(next)
    });
    match result {
        Ok(mut next) => {
            let accessory = match choice {
                WeaponUpgrade::Flashlight => {
                    Some(crate::weapon_upgrades::WeaponAccessory::Flashlight)
                }
                WeaponUpgrade::Laser => Some(crate::weapon_upgrades::WeaponAccessory::Laser),
                _ => None,
            };
            if let Some(accessory) = accessory {
                next.set_accessory_enabled(accessory, true).unwrap();
            }
            // Debit synchronously with the state commit. Even if destruction
            // is deferred, a second queued install sees an exhausted device.
            let consumed = if let Payment::Device(device) = payment {
                let count = world
                    .borrow::<View<dark::properties::PropStackCount>>()
                    .ok()
                    .and_then(|v| v.get(device).ok().map(|s| s.0))
                    .unwrap_or(1);
                world.add_component(device, dark::properties::PropStackCount(count - 1));
                if count == 1 {
                    Effect::DestroyEntity { entity_id: device }
                } else {
                    Effect::NoEffect
                }
            } else {
                Effect::NoEffect
            };
            world.add_component(weapon, next);
            Effect::combine(vec![
                consumed,
                Effect::ShowMessage {
                    text: format!("Installed {}.", label(choice)),
                },
            ])
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
        install(
            &mut world,
            gun,
            WeaponUpgrade::ExtendedCapacity,
            0,
            Payment::Modify,
        );
        install(
            &mut world,
            gun,
            WeaponUpgrade::LowMaintenanceI,
            0,
            Payment::Modify,
        );
        assert_eq!(
            state(&world, gun).choices(),
            &[WeaponUpgrade::ExtendedCapacity]
        );
        install(
            &mut world,
            gun,
            WeaponUpgrade::LowMaintenanceI,
            1,
            Payment::Modify,
        );
        assert_eq!(state(&world, gun).tier(), 2);
        assert!(
            quote(&world, gun, WeaponUpgrade::LowMaintenanceII, 2)
                .unwrap_err()
                .contains("device")
        );
        install(
            &mut world,
            gun,
            WeaponUpgrade::LowMaintenanceII,
            2,
            Payment::Modify,
        );
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
        assert!(quote(&world, gun, WeaponUpgrade::Silencer, 0).is_ok());
        assert!(quote(&world, gun, WeaponUpgrade::LowMaintenanceII, 0).is_err());
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 0;
        install(
            &mut world,
            gun,
            WeaponUpgrade::ExtendedCapacity,
            0,
            Payment::Modify,
        );
        assert_eq!(state(&world, gun).tier(), 0);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 6;
        world.add_component(gun, dark::properties::PropObjState(ObjectState::Broken));
        install(
            &mut world,
            gun,
            WeaponUpgrade::ExtendedCapacity,
            0,
            Payment::Modify,
        );
        assert_eq!(state(&world, gun).tier(), 0);
        world.add_component(gun, dark::properties::PropObjState(ObjectState::Normal));
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        install(
            &mut world,
            gun,
            WeaponUpgrade::ExtendedCapacity,
            0,
            Payment::Modify,
        );
        assert_eq!(state(&world, gun).tier(), 0);
    }

    #[test]
    fn devices_bypass_training_consume_once_and_reach_four_without_extra_paid_slots() {
        let (mut world, gun) = fixture();
        let device = world.add_entity((
            PropScripts {
                scripts: vec!["FreeModify".into()],
                inherits: false,
            },
            dark::properties::PropStackCount(4),
        ));
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = Some(device);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 0;
        let count = |world: &World| {
            world
                .borrow::<View<dark::properties::PropStackCount>>()
                .unwrap()
                .get(device)
                .unwrap()
                .0
        };
        assert!(
            validate(
                &world,
                gun,
                WeaponUpgrade::LowMaintenanceII,
                0,
                Payment::Device(device)
            )
            .is_err()
        );
        assert_eq!(count(&world), 4);
        for (tier, choice) in [
            WeaponUpgrade::ExtendedCapacity,
            WeaponUpgrade::LowMaintenanceI,
            WeaponUpgrade::LowMaintenanceII,
            WeaponUpgrade::AlternateFire,
        ]
        .into_iter()
        .enumerate()
        {
            install(&mut world, gun, choice, tier, Payment::Device(device));
            assert_eq!(state(&world, gun).tier(), tier + 1);
            assert_eq!(count(&world), 3 - tier as i32);
            install(&mut world, gun, choice, tier, Payment::Device(device));
            assert_eq!(
                count(&world),
                3 - tier as i32,
                "repeated completion cannot consume again"
            );
        }
        assert!(quote(&world, gun, WeaponUpgrade::ExtendedCapacity, 4).is_err());
        assert!(
            !device_available(&world, device),
            "zero count is unavailable before deferred destruction"
        );
        assert!((state(&world, gun).wear_multiplier() - 0.4).abs() < 0.0001);
        assert!(alternate_unlocked(&world, gun));
    }

    #[test]
    fn missing_device_and_locked_saved_setting_cannot_bypass_installation() {
        let (mut world, gun) = fixture();
        {
            let mut guns = world.borrow::<shipyard::ViewMut<PropGunState>>().unwrap();
            (&mut guns).get(gun).unwrap().setting = 1;
        }
        assert_eq!(
            crate::scripts::script_util::current_gun_setting(&world, gun),
            0
        );
        assert!(!crate::scripts::script_util::can_cycle_gun_setting(
            &world, gun
        ));
        let device = world.add_entity((
            PropScripts {
                scripts: vec!["FreeModify".into()],
                inherits: false,
            },
            dark::properties::PropStackCount(1),
        ));
        install(
            &mut world,
            gun,
            WeaponUpgrade::AlternateFire,
            0,
            Payment::Device(device),
        );
        assert_eq!(
            state(&world, gun).tier(),
            0,
            "world devices cannot be spent remotely"
        );
        assert_eq!(
            world
                .borrow::<View<dark::properties::PropStackCount>>()
                .unwrap()
                .get(device)
                .unwrap()
                .0,
            1
        );
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = Some(device);
        install(
            &mut world,
            gun,
            WeaponUpgrade::AlternateFire,
            0,
            Payment::Device(device),
        );
        assert_eq!(
            crate::scripts::script_util::current_gun_setting(&world, gun),
            1
        );
    }
}
