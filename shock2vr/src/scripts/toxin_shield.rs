//! Neural Toxin-blocker (`Toxin Shield`, template -1113): while the sustained
//! power is active the caster shrugs off toxin.
//!
//! Unlike Immolate, the power authors **no** receptron links - its only
//! tuning is `PropPsiPower::data[0] = 100`, read here as a **percentage of
//! toxin resistance** (100 = total immunity), which is the reading the
//! published tier-3 description gives. The block is expressed as a synthetic
//! `Amplify` receptron on the toxin stim so it composes with the act/react
//! chain like any authored shield: whatever eventually consumes the toxin
//! stim (a damage receptron, or a poisoning status like radiation's) sees an
//! intensity already scaled to zero.

use dark::properties::{ReceptronEffect, ReceptronOptions};
use shipyard::{EntityId, UniqueView, World};

use crate::{
    mission::PlayerInfo,
    psi::{ActivePsiPowers, GlobalPsiPowers, TOXIN_SHIELD_TEMPLATE_ID},
};

/// `Venom` - the gamesys's toxin stimulus archetype. It is what every toxic
/// source in the shipped data emits (`WormGoo` and the arachnid claws by
/// contact, `EggGooCloud` and the `Goo Projectile` at radius), and the only
/// stim `The Player` answers with a `toxin` receptron.
pub const VENOM_STIM_TEMPLATE_ID: i32 = -387;

/// Where the shield sits in the receiver's receptron order. Inert today -
/// `resolve_stim_damage` applies every Amplify before any Damage regardless
/// of order - but it keeps the shield in the same 78-79 band the shipped
/// PsiShield/armor Amplify receptrons use.
const TOXIN_SHIELD_RECEPTRON_ORDER: i32 = 79;

/// Fallback resistance when the power's authored data is unavailable, in
/// percent. The gamesys authors 100 (total immunity).
const DEFAULT_TOXIN_RESISTANCE_PERCENT: f32 = 100.0;

/// The receptrons an active Toxin Shield grants `target`: an `Amplify` on the
/// toxin stim scaled by the power's authored resistance percentage. Retail
/// attaches the power as a metaproperty on the player; this is that, for a
/// power whose "metaproperty" carries data instead of links.
pub fn toxin_shield_caster_receptrons(
    world: &World,
    target: EntityId,
) -> Vec<(i32, ReceptronOptions)> {
    let is_player = world
        .borrow::<UniqueView<PlayerInfo>>()
        .is_ok_and(|player| player.entity_id == target);
    let active = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .is_ok_and(|powers| powers.is_active(TOXIN_SHIELD_TEMPLATE_ID));
    if !is_player || !active {
        return Vec::new();
    }

    let resistance = world
        .borrow::<UniqueView<GlobalPsiPowers>>()
        .ok()
        .and_then(|powers| {
            powers
                .0
                .iter()
                .find(|power| power.template_id == TOXIN_SHIELD_TEMPLATE_ID)
                .map(|power| power.power.data[0])
        })
        .unwrap_or(DEFAULT_TOXIN_RESISTANCE_PERCENT);

    vec![(
        VENOM_STIM_TEMPLATE_ID,
        ReceptronOptions {
            order: TOXIN_SHIELD_RECEPTRON_ORDER,
            effect: ReceptronEffect::Amplify {
                factor: (1.0 - resistance / 100.0).clamp(0.0, 1.0),
            },
        },
    )]
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{PropPsiPower, ReceptronEffect, ReceptronOptions};
    use shipyard::World;

    use crate::mission::stim_response::resolve_stim_damage;
    use crate::psi::{ActivePsiPower, PsiPowerInfo};

    use super::*;

    /// A toxin damage receptron of the shape the player would carry once the
    /// port answers the toxin stim. The shipped data gives `The Player` a
    /// `toxin` receptron the port does not implement yet, so this stands in
    /// for whatever consumes the stim - the shield's job is to zero the
    /// intensity before it gets there.
    fn toxin_damage_receptron() -> (i32, ReceptronOptions) {
        (
            VENOM_STIM_TEMPLATE_ID,
            ReceptronOptions {
                order: 30,
                effect: ReceptronEffect::Damage {
                    multiplier: 1.0,
                    use_intensity: true,
                },
            },
        )
    }

    fn world_with(resistance_percent: f32, active: bool) -> World {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(GlobalPsiPowers(vec![PsiPowerInfo {
            template_id: TOXIN_SHIELD_TEMPLATE_ID,
            name: "Toxin Shield".to_string(),
            display_name: None,
            power: PropPsiPower {
                power_id: 20,
                activation_type: 1,
                psi_cost: 3,
                data: [resistance_percent, 0.0, 0.0, 0.0],
            },
            projectiles: Vec::new(),
            overloadable: false,
            duration: None,
        }]));
        world.add_unique(ActivePsiPowers(if active {
            vec![ActivePsiPower {
                template_id: TOXIN_SHIELD_TEMPLATE_ID,
                name: "Toxin Shield".to_string(),
                remaining_secs: 35.0,
            }]
        } else {
            Vec::new()
        }));
        world
    }

    fn player_of(world: &World) -> EntityId {
        world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id
    }

    #[test]
    fn an_inactive_power_grants_nothing() {
        let world = world_with(100.0, false);
        let player = player_of(&world);
        assert!(toxin_shield_caster_receptrons(&world, player).is_empty());
    }

    #[test]
    fn only_the_caster_is_shielded() {
        let mut world = world_with(100.0, true);
        let bystander = world.add_entity(());
        assert!(toxin_shield_caster_receptrons(&world, bystander).is_empty());
    }

    #[test]
    fn the_authored_hundred_percent_blocks_all_toxin_damage() {
        let world = world_with(100.0, true);
        let player = player_of(&world);

        let mut receptrons = vec![toxin_damage_receptron()];
        assert_eq!(
            resolve_stim_damage(&receptrons, VENOM_STIM_TEMPLATE_ID, 10.0),
            Some(10.0),
            "without the shield a toxin stim lands in full"
        );

        receptrons.extend(toxin_shield_caster_receptrons(&world, player));
        assert_eq!(
            resolve_stim_damage(&receptrons, VENOM_STIM_TEMPLATE_ID, 10.0),
            Some(0.0)
        );
    }

    #[test]
    fn a_partial_resistance_scales_rather_than_blocks() {
        let world = world_with(40.0, true);
        let player = player_of(&world);

        let mut receptrons = vec![toxin_damage_receptron()];
        receptrons.extend(toxin_shield_caster_receptrons(&world, player));
        assert_eq!(
            resolve_stim_damage(&receptrons, VENOM_STIM_TEMPLATE_ID, 10.0),
            Some(6.0)
        );
    }

    #[test]
    fn other_stims_are_untouched() {
        const INCENDIARY: i32 = -388;
        let world = world_with(100.0, true);
        let player = player_of(&world);

        let mut receptrons = vec![(
            INCENDIARY,
            ReceptronOptions {
                order: 30,
                effect: ReceptronEffect::Damage {
                    multiplier: 1.0,
                    use_intensity: true,
                },
            },
        )];
        receptrons.extend(toxin_shield_caster_receptrons(&world, player));
        assert_eq!(
            resolve_stim_damage(&receptrons, INCENDIARY, 10.0),
            Some(10.0)
        );
    }
}
