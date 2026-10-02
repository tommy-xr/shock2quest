//! Active psionic metaproperties contribute their authored stimulus filters
//! to the player's receivers, before contact or radius damage is resolved.

use dark::{
    properties::{Link, ReceptronEffect, ReceptronOptions},
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use shipyard::{EntityId, Unique, UniqueView, World};

use crate::{
    mission::PlayerInfo,
    psi::{ActivePsiPowers, GlobalPsiPowers},
};

/// Verified defenses that use their authored stimulus filters. Screen already
/// scales final Damage, and Immolate has its existing consumer below.
const ENERGY_REFLECTION_TEMPLATE_ID: i32 = -3153;
const PSYCHO_REFLECTIVE_AURA_TEMPLATE_ID: i32 = -1019;

#[derive(Unique, Default)]
pub struct PowerReceptrons(Vec<(i32, Vec<(i32, ReceptronOptions)>)>);

impl PowerReceptrons {
    pub fn from_registry(info: &SystemShock2EntityInfo, powers: &GlobalPsiPowers) -> Self {
        Self::from_hierarchy(info, powers, ss2_entity_info::get_hierarchy(info))
    }

    fn from_hierarchy(
        info: &SystemShock2EntityInfo,
        powers: &GlobalPsiPowers,
        hierarchy: &std::collections::HashMap<i32, Vec<i32>>,
    ) -> Self {
        Self(
            powers
                .0
                .iter()
                .filter_map(|power| {
                    if !matches!(
                        power.template_id,
                        ENERGY_REFLECTION_TEMPLATE_ID | PSYCHO_REFLECTIVE_AURA_TEMPLATE_ID
                    ) {
                        return None;
                    }
                    let mut ancestors =
                        ss2_entity_info::get_ancestors(hierarchy, &power.template_id);
                    ancestors.push(power.template_id);
                    let receivers = ancestors
                        .into_iter()
                        .flat_map(|id| info.template_to_links.get(&id))
                        .flat_map(|links| &links.to_links)
                        .filter_map(|link| match &link.link {
                            Link::Receptron(options)
                                if matches!(
                                    options.effect,
                                    ReceptronEffect::Abort | ReceptronEffect::Amplify { .. }
                                ) =>
                            {
                                Some((link.to_template_id, options.clone()))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    (!receivers.is_empty()).then_some((power.template_id, receivers))
                })
                .collect(),
        )
    }
}

pub fn active_receptrons(world: &World, target: EntityId) -> Vec<(i32, ReceptronOptions)> {
    let mut receivers = crate::scripts::immolate::immolate_caster_receptrons(world, target);
    if !world
        .borrow::<UniqueView<PlayerInfo>>()
        .is_ok_and(|player| player.entity_id == target)
    {
        return receivers;
    }
    if let (Ok(active), Ok(authored)) = (
        world.borrow::<UniqueView<ActivePsiPowers>>(),
        world.borrow::<UniqueView<PowerReceptrons>>(),
    ) {
        for (_, links) in authored.0.iter().filter(|(id, _)| active.is_active(*id)) {
            receivers.extend(links.iter().cloned());
        }
    }
    receivers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::stim_response::resolve_stim_damage,
        psi::{
            ActivePsiPower, IMMOLATE_TEMPLATE_ID, PSYCHO_REFLECTIVE_SCREEN_TEMPLATE_ID,
            PsiPowerInfo,
        },
    };
    use cgmath::{Quaternion, vec3};
    use dark::properties::{PropPsiPower, TemplateLinks, ToTemplateLink};
    use shipyard::UniqueViewMut;
    use std::collections::HashMap;

    const POWER: i32 = -3153;
    const PSI: i32 = -389;
    const ENERGY: i32 = -373;
    fn receiver(stim: i32, effect: ReceptronEffect) -> (i32, ReceptronOptions) {
        (stim, ReceptronOptions { order: 77, effect })
    }
    fn power(id: i32) -> PsiPowerInfo {
        PsiPowerInfo {
            template_id: id,
            name: "fixture".into(),
            display_name: None,
            power: PropPsiPower {
                power_id: 19,
                activation_type: 1,
                psi_cost: 3,
                data: [0.0; 4],
            },
            projectiles: vec![],
            overloadable: false,
            duration: None,
        }
    }
    fn authored() -> Vec<(i32, ReceptronOptions)> {
        vec![
            receiver(PSI, ReceptronEffect::Abort),
            receiver(ENERGY, ReceptronEffect::Amplify { factor: 0.5 }),
        ]
    }
    fn world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let player = world.add_entity(());
        let other = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: other,
        });
        world.add_unique(ActivePsiPowers(vec![ActivePsiPower {
            template_id: POWER,
            name: "AntiPsi".into(),
            remaining_secs: 120.0,
        }]));
        world.add_unique(PowerReceptrons(vec![(POWER, authored())]));
        (world, player, other)
    }
    #[test]
    fn filters_apply_only_to_player_while_active() {
        let (world, player, other) = world();
        assert_eq!(active_receptrons(&world, player).len(), 2);
        assert!(active_receptrons(&world, other).is_empty());
        world
            .borrow::<UniqueViewMut<ActivePsiPowers>>()
            .unwrap()
            .0
            .clear();
        assert!(active_receptrons(&world, player).is_empty());
    }
    #[test]
    fn authored_abort_wins_over_damage_and_amplify_in_either_order() {
        let (world, player, _) = world();
        let mut links = active_receptrons(&world, player);
        links.extend([
            receiver(
                PSI,
                ReceptronEffect::Damage {
                    multiplier: 1.0,
                    use_intensity: true,
                },
            ),
            receiver(PSI, ReceptronEffect::Amplify { factor: 2.0 }),
        ]);
        assert_eq!(resolve_stim_damage(&links, PSI, 10.0), None);
        links.reverse();
        assert_eq!(resolve_stim_damage(&links, PSI, 10.0), None);
        links.push(receiver(
            ENERGY,
            ReceptronEffect::Damage {
                multiplier: 2.0,
                use_intensity: true,
            },
        ));
        assert_eq!(resolve_stim_damage(&links, ENERGY, 5.0), Some(5.0));
        links.push(receiver(
            -385,
            ReceptronEffect::Damage {
                multiplier: 1.0,
                use_intensity: true,
            },
        ));
        assert_eq!(resolve_stim_damage(&links, -385, 10.0), Some(10.0));
    }
    #[test]
    fn simultaneous_screen_and_immolate_keep_their_single_existing_consumers() {
        let (world, player, _) = world();
        let mut info = SystemShock2EntityInfo::empty();
        info.template_to_links.insert(
            IMMOLATE_TEMPLATE_ID,
            TemplateLinks {
                to_links: vec![ToTemplateLink {
                    to_template_id: -388,
                    link: Link::Receptron(ReceptronOptions {
                        order: 77,
                        effect: ReceptronEffect::Amplify { factor: 0.0 },
                    }),
                }],
            },
        );
        world.add_unique(crate::scripts::immolate::ImmolateAura::from_entity_info(
            &info,
        ));
        world.add_unique(crate::psi::PsychoReflectiveScreenFactor(0.85));
        world
            .borrow::<UniqueViewMut<ActivePsiPowers>>()
            .unwrap()
            .0
            .extend(
                [IMMOLATE_TEMPLATE_ID, PSYCHO_REFLECTIVE_SCREEN_TEMPLATE_ID].map(|id| {
                    ActivePsiPower {
                        template_id: id,
                        name: "fixture".into(),
                        remaining_secs: 30.0,
                    }
                }),
            );
        let mut links = active_receptrons(&world, player);
        assert_eq!(
            links.len(),
            3,
            "two AntiPsi filters and exactly one Immolate filter"
        );
        links.push(receiver(
            -388,
            ReceptronEffect::Damage {
                multiplier: 1.0,
                use_intensity: true,
            },
        ));
        assert_eq!(resolve_stim_damage(&links, -388, 10.0), Some(0.0));
        links.push(receiver(
            ENERGY,
            ReceptronEffect::Damage {
                multiplier: 2.0,
                use_intensity: true,
            },
        ));
        assert_eq!(resolve_stim_damage(&links, ENERGY, 5.0), Some(5.0));
        assert_eq!(crate::psi::screen_damage_factor(&world), 0.85);
    }

    #[test]
    fn aura_filters_all_nine_authored_stims_without_widening_other_defenses() {
        let (world, player, other) = world();
        let covered = [-2753, -373, -388, -377, -375, -376, -385, -3058, -1145];
        let mut info = SystemShock2EntityInfo::empty();
        info.template_to_links.insert(
            PSYCHO_REFLECTIVE_AURA_TEMPLATE_ID,
            TemplateLinks {
                to_links: covered
                    .map(|stim| ToTemplateLink {
                        to_template_id: stim,
                        link: Link::Receptron(ReceptronOptions {
                            order: 79,
                            effect: ReceptronEffect::Amplify { factor: 0.4 },
                        }),
                    })
                    .into(),
            },
        );
        *world.borrow::<UniqueViewMut<PowerReceptrons>>().unwrap() =
            PowerReceptrons::from_hierarchy(
                &info,
                &GlobalPsiPowers(vec![power(PSYCHO_REFLECTIVE_AURA_TEMPLATE_ID)]),
                &HashMap::new(),
            );
        world.borrow::<UniqueViewMut<ActivePsiPowers>>().unwrap().0 = vec![ActivePsiPower {
            template_id: PSYCHO_REFLECTIVE_AURA_TEMPLATE_ID,
            name: "PsiShield".into(),
            remaining_secs: 1.0,
        }];
        for stim in covered.into_iter().chain([-374, -389, -386]) {
            let mut responses = active_receptrons(&world, player);
            responses.push(receiver(
                stim,
                ReceptronEffect::Damage {
                    multiplier: 2.0,
                    use_intensity: true,
                },
            ));
            assert_eq!(
                resolve_stim_damage(&responses, stim, 10.0),
                Some(if covered.contains(&stim) { 8.0 } else { 20.0 })
            );
        }
        assert!(active_receptrons(&world, other).is_empty());
        world
            .borrow::<UniqueViewMut<ActivePsiPowers>>()
            .unwrap()
            .0
            .clear();
        assert!(active_receptrons(&world, player).is_empty());
    }

    #[test]
    fn inherited_filters_load_but_existing_consumers_are_not_duplicated() {
        let mut info = SystemShock2EntityInfo::empty();
        let links = authored()
            .into_iter()
            .map(|(stim, options)| ToTemplateLink {
                to_template_id: stim,
                link: Link::Receptron(options),
            })
            .collect();
        info.template_to_links
            .insert(-9000, TemplateLinks { to_links: links });
        let powers = GlobalPsiPowers(vec![
            power(POWER),
            power(IMMOLATE_TEMPLATE_ID),
            power(PSYCHO_REFLECTIVE_SCREEN_TEMPLATE_ID),
            power(-1019),
        ]);
        let hierarchy = HashMap::from([
            (POWER, vec![-9000]),
            (-1019, vec![-9000]),
            (IMMOLATE_TEMPLATE_ID, vec![-9000]),
            (PSYCHO_REFLECTIVE_SCREEN_TEMPLATE_ID, vec![-9000]),
        ]);
        let registry = PowerReceptrons::from_hierarchy(&info, &powers, &hierarchy);
        assert_eq!(registry.0.len(), 2);
        assert_eq!(registry.0[1].0, -1019);
        assert_eq!(registry.0[1].1.len(), 2);
        assert_eq!(registry.0[0].0, POWER);
        assert_eq!(registry.0[0].1.len(), 2);
    }
}
