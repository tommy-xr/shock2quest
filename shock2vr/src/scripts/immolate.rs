//! Localized Pyrokinesis (`Immolate`, template -3152): while the sustained
//! power is active the caster burns, so the aura lives here - beside the
//! mechanic it drives - rather than in the psi amp's cast path.
//!
//! Everything is read from the power's own gamesys links, not from constants:
//! `StimSource(intensity 5, Radius 4) -> Incendiary` is the damaging aura and
//! `Receptron(Amplify 0.0) -> Incendiary` is the caster's own fire immunity.

use dark::{
    properties::{Link, ReceptronOptions, StimPropagator},
    ss2_entity_info::SystemShock2EntityInfo,
};
use shipyard::{EntityId, Unique, UniqueView, UniqueViewMut, World};

use crate::{mission::PlayerInfo, psi::ActivePsiPowers, psi::IMMOLATE_TEMPLATE_ID};

use super::Effect;

/// How often the aura stimulates what stands in it. The shipped
/// `sStimSourceDesc` carries the source's period, but the port parses only the
/// propagator and intensity, so this is an **assumption**: one pulse per
/// second, which reads the authored intensity 5 as 5 damage per second at the
/// player (linear falloff to 0 at the 4-unit edge) and burns a 12-HP pipe
/// hybrid down in a few seconds of contact.
const IMMOLATE_PULSE_INTERVAL_SECS: f32 = 1.0;

/// The Immolate aura's authored data, hydrated once at level load: the
/// `StimSource` it emits and the receptrons it grants its caster.
#[derive(Unique, Default)]
pub struct ImmolateAura {
    /// `(stim archetype, intensity, radius)` of the power's radius StimSource.
    stim: Option<(i32, f32, f32)>,
    /// The power's own receptrons, applied to the caster while it is active.
    receptrons: Vec<(i32, ReceptronOptions)>,
    secs_since_pulse: f32,
}

impl ImmolateAura {
    pub fn from_entity_info(entity_info: &SystemShock2EntityInfo) -> Self {
        // The power's own links only: `Immolate`'s ancestors (`Level 2`,
        // `Psi Powers`, `MetaProperty`) author none.
        let Some(links) = entity_info.template_to_links.get(&IMMOLATE_TEMPLATE_ID) else {
            return Self::default();
        };

        let mut aura = Self::default();
        for link in &links.to_links {
            match &link.link {
                Link::StimSource(options) => {
                    if let StimPropagator::Radius { radius } = options.propagator {
                        aura.stim = Some((link.to_template_id, options.intensity, radius));
                    }
                }
                Link::Receptron(options) => {
                    aura.receptrons.push((link.to_template_id, options.clone()))
                }
                _ => {}
            }
        }
        aura
    }
}

/// Immolate's own receptrons, applied to `target` while the caster is burning:
/// its `Amplify 0.0` on Incendiary is what makes them fireproof. Retail
/// attaches the power as a metaproperty on the player; this is that, for the
/// one power that authors receptrons today.
pub fn immolate_caster_receptrons(world: &World, target: EntityId) -> Vec<(i32, ReceptronOptions)> {
    let is_player = world
        .borrow::<UniqueView<PlayerInfo>>()
        .is_ok_and(|player| player.entity_id == target);
    let active = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .is_ok_and(|powers| powers.is_active(IMMOLATE_TEMPLATE_ID));
    if !is_player || !active {
        return Vec::new();
    }
    world
        .borrow::<UniqueView<ImmolateAura>>()
        .map(|aura| aura.receptrons.clone())
        .unwrap_or_default()
}

/// Pulse the aura while Immolate is active: a radius stim centered on the
/// player, resolved through each victim's receptrons like any other stim -
/// including the player's own, whose Amplify 0.0 zeroes it (self-immunity).
pub fn tick_immolate_aura(world: &World, elapsed_secs: f32) -> Option<Effect> {
    let (stim_template_id, intensity, radius) =
        world.borrow::<UniqueView<ImmolateAura>>().ok()?.stim?;
    let active = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .ok()?
        .is_active(IMMOLATE_TEMPLATE_ID);

    let mut aura = world.borrow::<UniqueViewMut<ImmolateAura>>().ok()?;
    if !active {
        // Re-casting starts a fresh cycle, so the first pulse lands promptly.
        aura.secs_since_pulse = IMMOLATE_PULSE_INTERVAL_SECS;
        return None;
    }
    // Never bank more than one pulse: a long frame (a level load, a debugger
    // pause) must not fire a burst of them once stepping resumes.
    aura.secs_since_pulse =
        (aura.secs_since_pulse + elapsed_secs).min(2.0 * IMMOLATE_PULSE_INTERVAL_SECS);
    if aura.secs_since_pulse < IMMOLATE_PULSE_INTERVAL_SECS {
        return None;
    }
    drop(aura);

    // Read the player before consuming the interval, so a missing PlayerInfo
    // cannot swallow a pulse.
    let center = world.borrow::<UniqueView<PlayerInfo>>().ok()?.pos;
    world
        .borrow::<UniqueViewMut<ImmolateAura>>()
        .ok()?
        .secs_since_pulse -= IMMOLATE_PULSE_INTERVAL_SECS;
    Some(Effect::RadiusStim {
        center,
        radius,
        intensity,
        stim_template_id,
    })
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        Link, ReceptronEffect, ReceptronOptions, StimSourceOptions, TemplateLinks, ToTemplateLink,
    };
    use shipyard::World;

    use crate::mission::stim_response::resolve_stim_damage;
    use crate::psi::{ActivePsiPower, ActivePsiPowers};

    use super::*;

    /// The shipped `Incendiary` stim archetype.
    const INCENDIARY: i32 = -388;

    fn authored_aura() -> ImmolateAura {
        ImmolateAura {
            stim: Some((INCENDIARY, 5.0, 4.0)),
            receptrons: vec![(
                INCENDIARY,
                ReceptronOptions {
                    order: 77,
                    effect: ReceptronEffect::Amplify { factor: 0.0 },
                },
            )],
            secs_since_pulse: 0.0,
        }
    }

    fn world_with(aura: ImmolateAura, active: bool) -> World {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(aura);
        world.add_unique(ActivePsiPowers(if active {
            vec![ActivePsiPower {
                template_id: IMMOLATE_TEMPLATE_ID,
                name: "Immolate".to_string(),
                remaining_secs: 30.0,
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
    fn hydrates_the_authored_stim_and_receptron() {
        let mut entity_info = dark::ss2_entity_info::SystemShock2EntityInfo::empty();
        entity_info
            .entity_to_properties
            .insert(IMMOLATE_TEMPLATE_ID, vec![]);
        entity_info.template_to_links.insert(
            IMMOLATE_TEMPLATE_ID,
            TemplateLinks {
                to_links: vec![
                    ToTemplateLink {
                        to_template_id: INCENDIARY,
                        link: Link::StimSource(StimSourceOptions {
                            intensity: 5.0,
                            propagator: StimPropagator::Radius { radius: 4.0 },
                        }),
                    },
                    ToTemplateLink {
                        to_template_id: INCENDIARY,
                        link: Link::Receptron(ReceptronOptions {
                            order: 77,
                            effect: ReceptronEffect::Amplify { factor: 0.0 },
                        }),
                    },
                ],
            },
        );

        let aura = ImmolateAura::from_entity_info(&entity_info);
        assert_eq!(aura.stim, Some((INCENDIARY, 5.0, 4.0)));
        assert_eq!(aura.receptrons.len(), 1);
    }

    #[test]
    fn inactive_power_never_pulses() {
        let world = world_with(authored_aura(), false);
        for _ in 0..300 {
            assert!(tick_immolate_aura(&world, 1.0 / 60.0).is_none());
        }
    }

    #[test]
    fn active_power_pulses_the_authored_stim_once_per_interval() {
        let world = world_with(authored_aura(), true);
        let mut pulses = 0;
        // Three seconds in exact (binary-representable) quarter-second steps,
        // so the count is not at the mercy of float drift.
        for _ in 0..12 {
            if let Some(effect) = tick_immolate_aura(&world, 0.25) {
                assert!(matches!(
                    effect,
                    Effect::RadiusStim {
                        center,
                        radius: 4.0,
                        intensity: 5.0,
                        stim_template_id: INCENDIARY,
                    } if center == vec3(1.0, 2.0, 3.0)
                ));
                pulses += 1;
            }
        }
        assert_eq!(pulses, 3);
    }

    #[test]
    fn the_caster_is_fireproof_only_while_the_power_is_active() {
        let burns = (
            INCENDIARY,
            ReceptronOptions {
                order: 19,
                effect: ReceptronEffect::Damage {
                    multiplier: 1.0,
                    use_intensity: true,
                },
            },
        );
        assert_eq!(
            resolve_stim_damage(&[burns.clone()], INCENDIARY, 10.0),
            Some(10.0)
        );

        let world = world_with(authored_aura(), true);
        let mut receptrons = vec![burns.clone()];
        receptrons.extend(immolate_caster_receptrons(&world, player_of(&world)));
        assert_eq!(
            resolve_stim_damage(&receptrons, INCENDIARY, 10.0),
            Some(0.0),
            "Amplify 0.0 makes the burning caster fireproof"
        );

        let expired = world_with(authored_aura(), false);
        assert!(immolate_caster_receptrons(&expired, player_of(&expired)).is_empty());
    }
}
