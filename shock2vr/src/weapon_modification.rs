//! Classic paid modification. HRM owns the paid attempt; the effect applier
//! commits one sequential modification and saves the resulting gun properties.
use crate::{player_stats::Skill, quest_info::QuestInfo};
use dark::properties::{
    ObjectState, PropBaseGunDesc, PropGunKick, PropGunState, PropHackDiff, PropModify2Diff,
    PropModifyDiff, PropModifyText1, PropModifyText2, PropObjState, PropRequiredTechDesc,
    PropScripts,
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

/// What modifying `weapon` from `level` does: its authored `P$Modify1` /
/// `P$Modify2` text, else a summary of the applied effect.
pub fn description(world: &World, weapon: EntityId, level: i32) -> String {
    let authored = match level {
        0 => world
            .borrow::<View<PropModifyText1>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|p| ("modify1", p.0.clone()))),
        1 => world
            .borrow::<View<PropModifyText2>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|p| ("modify2", p.0.clone()))),
        _ => None,
    };
    authored
        .map(|(table, raw)| crate::scripts::gui::PanelText::object_string(world, table, &raw))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| effect_summary(world, weapon, level).to_owned())
}

fn effect_summary(world: &World, weapon: EntityId, level: i32) -> &'static str {
    match (kind(world, weapon), Some(level)) {
        (Some(Kind::Pistol), Some(0)) => "+12 clip capacity; +10% damage.",
        (Some(Kind::Pistol), Some(1)) => "Reload time divided by three; +14% damage.",
        (Some(Kind::Rifle | Kind::Shotgun), Some(0)) => {
            "Reload time divided by three; +10% damage."
        }
        (Some(Kind::Shotgun), Some(1)) => "Recoil divided by three; +14% damage.",
        (Some(Kind::Rifle), Some(1)) => "+36 clip capacity; +14% damage.",
        (Some(Kind::Laser | Kind::Emp), Some(0)) => "+50 energy capacity; +10% damage.",
        (Some(Kind::Fusion), Some(0)) => "+40 energy capacity; +10% damage.",
        (Some(Kind::Annelid | Kind::Viral), Some(0)) => "+10 clip capacity; +10% damage.",
        (Some(Kind::Stasis), Some(0)) => "Double projectile speed.",
        (Some(Kind::Stasis), Some(1)) => "Half energy use.",
        (Some(Kind::Grenade), Some(0)) => "+3 clip capacity; increased damage.",
        (Some(Kind::Grenade), Some(1)) => "Faster projectiles and reload; +14% damage.",
        (Some(Kind::Annelid), Some(1)) => "Double projectile speed; +14% damage.",
        (Some(Kind::Emp), Some(1)) => "Faster projectiles, half energy use; +14% damage.",
        (Some(_), Some(1)) => "Less ammunition or energy per shot; +14% damage.",
        _ => "Weapon modification complete.",
    }
}

pub fn quote(world: &World, weapon: EntityId) -> Result<PropHackDiff, String> {
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

/// Original RootModify modifies settings 0 and 1 only. AddInt, MultiplyFloat
/// and DivideInt operations verified against September 1999 allobjs.osm
/// (Modify1/2 handlers at 0x100261fc–0x100270ec), also matching the remaster.
fn modify_desc(desc: &mut PropBaseGunDesc, kind: Kind, next: i32) {
    for (mode, s) in desc.settings[..2].iter_mut().enumerate() {
        match (kind, next) {
            (Kind::Pistol, 1) => s.clip += 12,
            (Kind::Pistol, 2) => s.reload_time_ms /= 3,
            (Kind::Shotgun | Kind::Rifle, 1) => s.reload_time_ms /= 3,
            (Kind::Rifle, 2) => s.clip += 36,
            (Kind::Laser | Kind::Emp, 1) => s.clip += 50,
            (Kind::Laser, 2) => s.ammo_usage = if mode == 0 { 2 } else { 14 },
            (Kind::Emp, 2) => {
                s.speed_modifier *= 1.5;
                s.ammo_usage /= 2;
            }
            (Kind::Fusion, 1) => s.clip += 40,
            (Kind::Fusion | Kind::Stasis | Kind::Viral, 2) => s.ammo_usage /= 2,
            (Kind::Stasis, 1) => s.speed_modifier *= 2.0,
            (Kind::Grenade, 1) => {
                s.clip += 3;
                s.stim_modifier = (s.stim_modifier as i32 + 1) as f32;
            }
            (Kind::Grenade, 2) => {
                s.speed_modifier *= 1.5;
                s.reload_time_ms /= 3;
            }
            (Kind::Annelid | Kind::Viral, 1) => s.clip += 10,
            (Kind::Annelid, 2) => s.speed_modifier *= 2.0,
            _ => {}
        }
        if kind != Kind::Stasis && !(kind == Kind::Grenade && next == 1) {
            s.stim_modifier *= if next == 1 { 1.1 } else { 1.14 };
        }
    }
}

pub fn apply(world: &mut World, weapon: EntityId, expected_level: i32) -> bool {
    // A stale/repeated success cannot advance a second level or mutate another gun.
    if level(world, weapon) != Some(expected_level) || quote(world, weapon).is_err() {
        return false;
    }
    let Some(kind) = kind(world, weapon) else {
        return false;
    };
    let Some(mut desc) = world
        .borrow::<View<PropBaseGunDesc>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().cloned())
    else {
        return false;
    };
    let next = expected_level + 1;
    modify_desc(&mut desc, kind, next);
    let mut state = world
        .borrow::<View<PropGunState>>()
        .unwrap()
        .get(weapon)
        .unwrap()
        .clone();
    state.modification = next;
    world.add_component(weapon, (desc, state));
    if kind == Kind::Shotgun && next == 2 {
        let kick = world
            .borrow::<View<PropGunKick>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().cloned());
        if let Some(mut kick) = kick {
            for s in &mut kick.settings[..2] {
                s.kick_pitch_degrees /= 3.0;
                s.kick_heading_degrees /= 3.0;
                s.kick_back /= 3.0;
                s.jolt_pitch_degrees /= 3.0;
                s.jolt_heading_degrees /= 3.0;
                s.jolt_back /= 3.0;
            }
            world.add_component(weapon, kick);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::GunSettingDesc;
    #[test]
    fn modifications_are_sequential_and_leave_unused_setting_alone() {
        let base = GunSettingDesc {
            clip: 12,
            reload_time_ms: 900,
            ..Default::default()
        };
        let mut desc = PropBaseGunDesc {
            settings: std::array::from_fn(|_| base.clone()),
        };
        modify_desc(&mut desc, Kind::Pistol, 1);
        assert_eq!(desc.settings[0].clip, 24);
        assert_eq!(desc.settings[1].stim_modifier, 1.1);
        modify_desc(&mut desc, Kind::Pistol, 2);
        assert_eq!(desc.settings[0].reload_time_ms, 300);
        assert!((desc.settings[0].stim_modifier - 1.254).abs() < 0.0001);
        assert_eq!(desc.settings[2], base);
    }
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
        assert_eq!(quote(&world, gun).unwrap().cost, 21.0);
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .add_os_trait(15);
        assert_eq!(quote(&world, gun).unwrap().cost, 10.0);
        assert!(apply(&mut world, gun, 0));
        assert!(!apply(&mut world, gun, 0));
        assert!(quote(&world, gun).unwrap_err().contains("Modify skill 4"));
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .modify = 4;
        assert_eq!(quote(&world, gun).unwrap().cost, 1.0);
        assert!(apply(&mut world, gun, 1));
        assert!(!apply(&mut world, gun, 1));
        assert!(
            quote(&world, gun)
                .unwrap_err()
                .contains("modified any further")
        );
        world
            .borrow::<shipyard::UniqueViewMut<crate::mission::PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        assert!(quote(&world, gun).is_err());
    }
    #[test]
    fn classic_weapon_families_keep_their_distinct_modification_effects() {
        // Expected end states from the ten retail OSM handlers, on deliberately
        // unequal mode inputs; catches accidentally copying mode 0 into mode 1.
        for (kind, clip_add, speed, ammo, reload, damage) in [
            (Kind::Pistol, 12, 1.0, [8, 16], 300, 1.254),
            (Kind::Shotgun, 0, 1.0, [8, 16], 300, 1.254),
            (Kind::Rifle, 36, 1.0, [8, 16], 300, 1.254),
            (Kind::Laser, 50, 1.0, [2, 14], 900, 1.254),
            (Kind::Emp, 50, 1.5, [4, 8], 900, 1.254),
            (Kind::Fusion, 40, 1.0, [4, 8], 900, 1.254),
            (Kind::Stasis, 0, 2.0, [4, 8], 900, 1.0),
            (Kind::Grenade, 3, 1.5, [8, 16], 300, 2.28),
            (Kind::Annelid, 10, 2.0, [8, 16], 900, 1.254),
            (Kind::Viral, 10, 1.0, [4, 8], 900, 1.254),
        ] {
            let mut desc = PropBaseGunDesc {
                settings: std::array::from_fn(|mode| GunSettingDesc {
                    clip: 12 + mode as i32,
                    ammo_usage: 8 * (mode as i32 + 1),
                    reload_time_ms: 900,
                    ..Default::default()
                }),
            };
            modify_desc(&mut desc, kind, 1);
            modify_desc(&mut desc, kind, 2);
            for (mode, setting) in desc.settings[..2].iter().enumerate() {
                assert_eq!(setting.clip, 12 + mode as i32 + clip_add, "{kind:?}");
                assert_eq!(setting.ammo_usage, ammo[mode], "{kind:?}");
                assert_eq!(setting.reload_time_ms, reload, "{kind:?}");
                assert_eq!(setting.speed_modifier, speed, "{kind:?}");
                assert!((setting.stim_modifier - damage).abs() < 0.0001, "{kind:?}");
            }
        }
    }
}
