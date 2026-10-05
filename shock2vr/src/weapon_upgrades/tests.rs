use super::*;

fn upgraded(choices: &[WeaponUpgrade]) -> WeaponUpgrades {
    choices
        .iter()
        .fold(WeaponUpgrades::default(), |state, choice| {
            state
                .with_upgrade(
                    *choice,
                    &WeaponUpgrade::ALL,
                    UpgradeSource::Device,
                    state.tier(),
                )
                .unwrap()
        })
}

#[test]
fn bonuses_are_additive_and_maintenance_two_replaces_one() {
    let mut state = WeaponUpgrades::default();
    for (choice, damage, wear) in [
        (WeaponUpgrade::ExtendedCapacity, 1.08, 0.95),
        (WeaponUpgrade::AlternateFire, 1.16, 0.90),
        (WeaponUpgrade::LowMaintenanceI, 1.24, 0.85 * 0.75),
        (WeaponUpgrade::LowMaintenanceII, 1.32, 0.80 * 0.50),
    ] {
        state = state
            .with_upgrade(
                choice,
                &WeaponUpgrade::ALL,
                UpgradeSource::Device,
                state.tier(),
            )
            .unwrap();
        assert!((state.damage_multiplier() - damage).abs() < 0.00001);
        assert!((state.wear_multiplier() - wear).abs() < 0.00001);
    }
}

#[test]
fn installation_checks_sequence_source_and_family_without_mutating_the_weapon() {
    use UpgradeError::*;
    use WeaponUpgrade::*;
    let empty = WeaponUpgrades::default();
    assert_eq!(
        empty.can_install(Silencer, &[Flashlight], UpgradeSource::Modify, 0),
        Err(Unsupported)
    );
    assert_eq!(
        empty.can_install(
            LowMaintenanceII,
            &WeaponUpgrade::ALL,
            UpgradeSource::Modify,
            0
        ),
        Err(RequiresLowMaintenanceI)
    );
    let first = empty
        .with_upgrade(Flashlight, &WeaponUpgrade::ALL, UpgradeSource::Device, 0)
        .unwrap();
    assert_eq!(empty.tier(), 0, "preview must not consume a slot");
    assert_eq!(
        first.can_install(Laser, &WeaponUpgrade::ALL, UpgradeSource::Modify, 0),
        Err(StaleTier)
    );
    assert_eq!(
        first.can_install(Flashlight, &WeaponUpgrade::ALL, UpgradeSource::Modify, 1),
        Err(AlreadyInstalled)
    );
    let second = first
        .with_upgrade(Laser, &WeaponUpgrade::ALL, UpgradeSource::Modify, 1)
        .unwrap();
    assert_eq!(
        second.can_install(Silencer, &WeaponUpgrade::ALL, UpgradeSource::Modify, 2),
        Err(RequiresDevice)
    );
    let third = second
        .with_upgrade(Silencer, &WeaponUpgrade::ALL, UpgradeSource::Device, 2)
        .unwrap();
    let fourth = third
        .with_upgrade(
            ExtendedCapacity,
            &WeaponUpgrade::ALL,
            UpgradeSource::Device,
            3,
        )
        .unwrap();
    assert_eq!(
        fourth.can_install(AlternateFire, &WeaponUpgrade::ALL, UpgradeSource::Device, 4),
        Err(MaximumTier)
    );
    assert_eq!(
        fourth.choices(),
        &[Flashlight, Laser, Silencer, ExtendedCapacity]
    );
}

#[test]
fn evaluation_preserves_mode_differences_and_does_not_reapply_retail_bonuses() {
    let state = upgraded(&[
        WeaponUpgrade::ExtendedCapacity,
        WeaponUpgrade::AlternateFire,
    ]);
    for (clip, damage, ammo) in [(12, 1.0, 1), (18, 2.0, 3)] {
        let base = GunSettingDesc {
            clip,
            stim_modifier: damage,
            ammo_usage: ammo,
            reload_time_ms: 900,
            speed_modifier: 1.5,
            ..Default::default()
        };
        let result = state.effective_setting(&base, true);
        assert_eq!(result.clip, clip * 2);
        assert!((result.stim_modifier - damage * 1.16).abs() < 0.00001);
        assert_eq!(result.ammo_usage, ammo);
        assert_eq!(result.reload_time_ms, 900);
        assert_eq!(result.speed_modifier, 1.5);
        assert_eq!(result, state.effective_setting(&base, true));
        assert_eq!(base.clip, clip, "evaluation must preserve authored input");
        assert_eq!(state.effective_setting(&base, false).stim_modifier, damage);
    }
    for clip in [0, -1] {
        let base = GunSettingDesc {
            clip,
            ..Default::default()
        };
        assert_eq!(state.effective_setting(&base, true).clip, clip);
    }
}

#[test]
fn toggles_require_installed_accessories_and_never_change_tier() {
    let mut state = WeaponUpgrades::default();
    assert_eq!(
        state.set_accessory_enabled(WeaponAccessory::Laser, true),
        Err(UpgradeError::NotInstalled)
    );
    state = upgraded(&[WeaponUpgrade::Laser, WeaponUpgrade::Flashlight]);
    state
        .set_accessory_enabled(WeaponAccessory::Laser, true)
        .unwrap();
    assert!(state.accessory_enabled(WeaponAccessory::Laser));
    assert!(!state.accessory_enabled(WeaponAccessory::Flashlight));
    state
        .set_accessory_enabled(WeaponAccessory::Laser, false)
        .unwrap();
    assert_eq!(state.tier(), 2);
    assert!(!state.accessory_enabled(WeaponAccessory::Laser));
}

#[test]
fn deserialization_rejects_states_installation_could_not_create() {
    for choices in [
        vec!["LowMaintenanceII"],
        vec!["Laser", "Laser"],
        vec![
            "Laser",
            "Flashlight",
            "Silencer",
            "ExtendedCapacity",
            "AlternateFire",
        ],
    ] {
        assert!(
            serde_json::from_value::<WeaponUpgrades>(serde_json::json!({
                "choices": choices, "flashlight_enabled": false, "laser_enabled": false
            }))
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<WeaponUpgrades>(serde_json::json!({
            "choices": [], "flashlight_enabled": false, "laser_enabled": true
        }))
        .is_err()
    );
}

#[test]
fn upgrade_choices_survive_save_partitioning_and_entity_remapping() {
    use crate::mission::{GlobalTemplateIdMap, PlayerInfo};
    use crate::runtime_props::RuntimePropDoNotSerialize;
    use crate::save_load::EntitySaveData;
    use shipyard::{Get, View, World};
    use std::collections::HashMap;

    let mut source = World::new();
    let player = source.add_entity(RuntimePropDoNotSerialize);
    let inventory = source.add_entity(());
    let mut carried_state = upgraded(&[
        WeaponUpgrade::Laser,
        WeaponUpgrade::Flashlight,
        WeaponUpgrade::LowMaintenanceI,
        WeaponUpgrade::LowMaintenanceII,
    ]);
    carried_state
        .set_accessory_enabled(WeaponAccessory::Laser, true)
        .unwrap();
    let ground_state = upgraded(&[WeaponUpgrade::ExtendedCapacity]);
    let carried = source.add_entity(carried_state.clone());
    let ground = source.add_entity(ground_state.clone());
    let filtered = source.add_entity((carried_state.clone(), RuntimePropDoNotSerialize));
    source.add_unique(GlobalTemplateIdMap(HashMap::new()));
    source.add_unique(PlayerInfo {
        pos: cgmath::vec3(0.0, 0.0, 0.0),
        rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        entity_id: player,
        inventory_entity_id: inventory,
        left_hand_entity_id: Some(carried),
        right_hand_entity_id: None,
    });
    let (world_save, held_save) = crate::save_load::to_save_data(&source);
    for (saved, original, expected, excluded) in [
        (world_save, ground, ground_state, carried),
        (held_save.held_entities, carried, carried_state, ground),
    ] {
        assert_eq!(saved.weapon_upgrades.len(), 1);
        assert!(!saved.all_entities.contains(&excluded.inner()));
        assert!(!saved.all_entities.contains(&filtered.inner()));
        let decoded: EntitySaveData =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let mut restored = World::new();
        for _ in 0..20 {
            restored.add_entity(());
        }
        let (_, remapped) = decoded.instantiate(&mut restored);
        assert_ne!(remapped[&original], original);
        assert_eq!(
            *restored
                .borrow::<View<WeaponUpgrades>>()
                .unwrap()
                .get(remapped[&original])
                .expect("weapon upgrades must survive save/load"),
            expected
        );
    }
}
