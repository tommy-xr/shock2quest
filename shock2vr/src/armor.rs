//! Worn armor contributes its authored hazard protection and Armor Effect
//! receptrons. Carrying an item alone never grants protection.
use crate::{mission::PlayerInfo, runtime_props::RuntimePropHazardEquipment};
use dark::{
    properties::{
        Link, Links, ObjectState, PropArmor, PropObjState, PropRequiredStats, ReceptronOptions,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use shipyard::{EntityId, Get, Unique, UniqueView, View, World};
use std::collections::HashMap;

#[derive(Unique, Default)]
pub struct ArmorEffects(HashMap<i32, Vec<(i32, ReceptronOptions)>>);
impl ArmorEffects {
    pub fn from_entity_info(info: &SystemShock2EntityInfo) -> Self {
        let hierarchy = ss2_entity_info::get_hierarchy(info);
        let mut effects = HashMap::new();
        for target in info
            .template_to_links
            .values()
            .flat_map(|l| &l.to_links)
            .filter(|l| matches!(l.link, Link::ArmorEffect))
            .map(|l| l.to_template_id)
        {
            effects.entry(target).or_insert_with(|| {
                let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &target);
                ancestors.push(target);
                ancestors
                    .into_iter()
                    .flat_map(|id| info.template_to_links.get(&id))
                    .flat_map(|l| &l.to_links)
                    .filter_map(|l| match &l.link {
                        Link::Receptron(options) => Some((l.to_template_id, options.clone())),
                        _ => None,
                    })
                    .collect()
            });
        }
        Self(effects)
    }
}

pub fn equipped(world: &World) -> Option<EntityId> {
    let (markers, armors) = world
        .borrow::<(View<RuntimePropHazardEquipment>, View<PropArmor>)>()
        .ok()?;
    crate::scripts::script_util::player_carried_items(world)
        .into_iter()
        .find(|id| markers.contains(*id) && armors.contains(*id))
}

/// A release also sends Drop when stowing in the backpack. Only the settled
/// ownership state can distinguish that from leaving the player's possession.
pub fn needs_unequip(world: &World, id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropHazardEquipment>>()
        .is_ok_and(|v| v.contains(id))
        && !crate::scripts::script_util::player_carried_items(world).contains(&id)
}

pub fn powered(world: &World, id: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, id, "PoweredArmor")
}
pub fn worm(world: &World, id: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, id, "WormSkin")
}
pub fn active(world: &World) -> Option<EntityId> {
    equipped(world).filter(|id| {
        let usable = !world.borrow::<View<PropObjState>>().is_ok_and(|v| {
            v.get(*id).is_ok_and(|s| {
                matches!(
                    s.0,
                    ObjectState::Unresearched | ObjectState::Broken | ObjectState::Destroyed
                )
            })
        });
        usable && (!powered(world, *id) || crate::implants::energy(world, *id) > 0.0)
    })
}

pub fn protection(world: &World) -> PropArmor {
    active(world)
        .and_then(|id| {
            world
                .borrow::<View<PropArmor>>()
                .ok()?
                .get(id)
                .ok()
                .copied()
        })
        .unwrap_or_default()
}

pub fn validate_equip(world: &World, id: EntityId) -> Result<(), String> {
    if !crate::scripts::script_util::player_carried_items(world).contains(&id) {
        return Err("Pick up the armor first.".into());
    }
    if !world
        .borrow::<View<PropArmor>>()
        .is_ok_and(|v| v.contains(id))
    {
        return Err("This item is not armor.".into());
    }
    if let Ok(states) = world.borrow::<View<PropObjState>>() {
        if let Ok(state) = states.get(id) {
            match state.0 {
                ObjectState::Unresearched => {
                    return Err("Research this armor before equipping it.".into());
                }
                ObjectState::Broken | ObjectState::Destroyed => {
                    return Err("This armor is broken.".into());
                }
                _ => {}
            }
        }
    }
    if let (Ok(requirements), Some(stats)) = (
        world.borrow::<View<PropRequiredStats>>(),
        crate::implants::effective_stats(world),
    ) {
        if let Ok(required) = requirements.get(id) {
            let values = [
                stats.strength,
                stats.endurance,
                stats.psionic_ability,
                stats.agility,
                stats.cyber_affinity,
            ];
            for ((actual, required), name) in values.into_iter().zip(required.0).zip([
                "Strength",
                "Endurance",
                "PSI",
                "Agility",
                "Cyber Affinity",
            ]) {
                if actual < required {
                    return Err(format!("Requires {name} {required}."));
                }
            }
        }
    }
    Ok(())
}

pub fn active_receptrons(world: &World, target: EntityId) -> Vec<(i32, ReceptronOptions)> {
    if !world
        .borrow::<UniqueView<PlayerInfo>>()
        .is_ok_and(|p| p.entity_id == target)
    {
        return vec![];
    }
    let Some(id) = active(world) else {
        return vec![];
    };
    let Ok((links, effects)) = world.borrow::<(View<Links>, UniqueView<ArmorEffects>)>() else {
        return vec![];
    };
    let Ok(links) = links.get(id) else {
        return vec![];
    };
    links
        .to_links
        .iter()
        .filter(|l| matches!(l.link, Link::ArmorEffect))
        .filter_map(|l| effects.0.get(&l.to_template_id))
        .flatten()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::stim_response::{GlobalContactStims, contact_stim_damage},
        quest_info::QuestInfo,
    };
    use dark::properties::{PropEnergy, PropScripts, ReceptronEffect, ToLink, WrappedEntityId};

    fn fixture(script: &str) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let armor = world.add_entity((
            PropArmor {
                toxic: 30.0,
                radiation: 30.0,
                combat: 20.0,
            },
            PropScripts {
                scripts: vec![script.into()],
                inherits: false,
            },
            PropEnergy(100.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: -900,
                    to_entity_id: None,
                    link: Link::ArmorEffect,
                }],
            },
        ));
        let player = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(armor)),
                    link: Link::Contains(0),
                },
                ToLink {
                    to_template_id: -385,
                    to_entity_id: None,
                    link: Link::Receptron(ReceptronOptions {
                        order: 15,
                        effect: ReceptronEffect::Damage {
                            multiplier: 1.0,
                            use_intensity: true,
                        },
                    }),
                },
            ],
        });
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalContactStims(HashMap::from([(
            -500,
            vec![(-385, 10.0)],
        )])));
        world.add_unique(ArmorEffects(HashMap::from([(
            -900,
            vec![(
                -385,
                ReceptronOptions {
                    order: 90,
                    effect: ReceptronEffect::Amplify { factor: 0.8 },
                },
            )],
        )])));
        (world, armor, player)
    }

    #[test]
    fn damage_and_hazards_require_wearing_owned_powered_armor() {
        let (mut world, armor, player) = fixture("PoweredArmor");
        assert_eq!(contact_stim_damage(&world, -500, player), 10.0);
        assert_eq!(protection(&world).radiation, 0.0);
        world.add_component(armor, RuntimePropHazardEquipment);
        assert_eq!(contact_stim_damage(&world, -500, player), 8.0);
        assert_eq!(protection(&world).radiation, 30.0);
        assert!(active_receptrons(&world, armor).is_empty());
        world.add_component(armor, PropEnergy(0.0));
        assert_eq!(contact_stim_damage(&world, -500, player), 10.0);
        assert_eq!(protection(&world).radiation, 0.0);
        world.add_component(armor, PropEnergy(100.0));
        world.add_component(player, Links::empty());
        assert!(needs_unequip(&world, armor));
        assert_eq!(equipped(&world), None);
        assert_eq!(protection(&world).combat, 0.0);
    }

    #[test]
    fn worm_bonus_is_derived_and_research_and_stat_requirements_are_enforced() {
        let (mut world, armor, _) = fixture("WormSkin");
        world.add_component(armor, PropObjState(ObjectState::Unresearched));
        assert!(validate_equip(&world, armor).is_err());
        world.add_component(armor, PropObjState(ObjectState::Normal));
        world.add_component(armor, PropRequiredStats([2, 0, 0, 0, 0]));
        assert_eq!(
            validate_equip(&world, armor),
            Err("Requires Strength 2.".into())
        );
        world
            .borrow::<shipyard::UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .strength = 2;
        assert!(validate_equip(&world, armor).is_ok());
        let base = crate::implants::effective_stats(&world)
            .unwrap()
            .psionic_ability;
        world.add_component(armor, RuntimePropHazardEquipment);
        assert_eq!(
            crate::implants::effective_stats(&world)
                .unwrap()
                .psionic_ability,
            base + 2
        );
        world.remove::<RuntimePropHazardEquipment>(armor);
        assert_eq!(
            crate::implants::effective_stats(&world)
                .unwrap()
                .psionic_ability,
            base
        );
    }

    #[test]
    fn armor_effect_relation_loads_authored_filters_including_multiple_energy_filters() {
        use dark::properties::{TemplateLinks, ToTemplateLink};
        let mut info = SystemShock2EntityInfo::empty();
        info.template_to_links.insert(
            -82,
            TemplateLinks {
                to_links: vec![ToTemplateLink {
                    to_template_id: -3488,
                    link: Link::ArmorEffect,
                }],
            },
        );
        info.template_to_links.insert(
            -3488,
            TemplateLinks {
                to_links: [89, 90]
                    .map(|order| ToTemplateLink {
                        to_template_id: -373,
                        link: Link::Receptron(ReceptronOptions {
                            order,
                            effect: ReceptronEffect::Amplify { factor: 0.5 },
                        }),
                    })
                    .into(),
            },
        );
        let effects = ArmorEffects::from_entity_info(&info);
        let mut receivers = effects.0[&-3488].clone();
        receivers.push((
            -373,
            ReceptronOptions {
                order: 15,
                effect: ReceptronEffect::Damage {
                    multiplier: 1.0,
                    use_intensity: true,
                },
            },
        ));
        assert_eq!(
            crate::mission::stim_response::resolve_stim_damage(&receivers, -373, 20.0),
            Some(5.0)
        );
    }
}
