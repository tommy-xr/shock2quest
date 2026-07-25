use std::collections::HashMap;

use dark::{
    properties::{Link, Links, ReceptronEffect, ReceptronOptions, StimPropagator},
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use shipyard::{EntityId, Get, Unique, UniqueView, View, World};

/// Every archetype's effective `Contact` stim sources - the `L$arSrcDesc`
/// links whose propagator is `Contact`, inherited down the metaproperty
/// hierarchy the same way properties are - as `(stim archetype, intensity)`.
///
/// A contact stim fires when the emitting object touches something. For an
/// AI's melee weapon (the hybrid's `Lead Pipe`, a `Rumbler Claw`, a `Midwife
/// Spike`) that touch *is* the swing landing, so these links are where the
/// gamesys authors AI melee damage: `Lead Pipe -> WeaponBash @ 10`, `Rumbler
/// Claw -> WeaponBash @ 20`, `Baby Arachnid Claw -> WeaponBash @ 2`, etc.
///
/// Keyed by template id rather than entity id because these archetypes are
/// never instantiated - a melee weapon is a virtual object hanging off the
/// creature's `L$Weapon` link.
#[derive(Unique, Clone, Default)]
pub struct GlobalContactStims(pub HashMap<i32, Vec<(i32, f32)>>);

impl GlobalContactStims {
    pub fn from_entity_info(entity_info: &SystemShock2EntityInfo) -> Self {
        let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
        let mut contact_stims = HashMap::new();

        for template_id in entity_info.entity_to_properties.keys() {
            let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, template_id);
            ancestors.push(*template_id);
            let mut stims: Vec<(i32, f32)> = Vec::new();
            for ancestor in ancestors {
                let Some(links) = entity_info.template_to_links.get(&ancestor) else {
                    continue;
                };
                for link in &links.to_links {
                    let Link::StimSource(options) = &link.link else {
                        continue;
                    };
                    if options.propagator != StimPropagator::Contact {
                        continue;
                    }
                    // A child archetype re-authoring the same stim replaces the
                    // inherited intensity rather than stacking with it.
                    match stims.iter_mut().find(|(stim, _)| *stim == link.to_template_id) {
                        Some(existing) => existing.1 = options.intensity,
                        None => stims.push((link.to_template_id, options.intensity)),
                    }
                }
            }
            if !stims.is_empty() {
                contact_stims.insert(*template_id, stims);
            }
        }

        Self(contact_stims)
    }
}

/// The damage `emitter_template`'s contact stims deal to `victim`, resolved
/// through the victim's own receptrons (so vulnerabilities, armor Amplify
/// receptrons and outright immunities all apply exactly as they do for
/// explosions and projectiles). Contributions from every contact stim the
/// emitter carries are summed; a victim with no receptron for a stim simply
/// feels nothing from it.
pub fn contact_stim_damage(world: &World, emitter_template: i32, victim: EntityId) -> f32 {
    let Ok(contact_stims) = world.borrow::<UniqueView<GlobalContactStims>>() else {
        return 0.0;
    };
    let Some(stims) = contact_stims.0.get(&emitter_template) else {
        return 0.0;
    };

    let receptrons = victim_receptrons(world, victim);
    stims
        .iter()
        .filter_map(|(stim_template_id, intensity)| {
            resolve_stim_damage(&receptrons, *stim_template_id, *intensity)
        })
        .filter(|damage| *damage > 0.0)
        .sum()
}

fn victim_receptrons(world: &World, victim: EntityId) -> Vec<(i32, ReceptronOptions)> {
    let Ok(v_links) = world.borrow::<View<Links>>() else {
        return Vec::new();
    };
    let Ok(links) = v_links.get(victim) else {
        return Vec::new();
    };
    links
        .to_links
        .iter()
        .filter_map(|link| match &link.link {
            Link::Receptron(options) => Some((link.to_template_id, options.clone())),
            _ => None,
        })
        .collect()
}

/// Resolve what damage a stim deals to a receiver, given the receiver's
/// receptron links (as `(stim_template_id, options)` pairs, i.e. the entity's
/// flattened `Link::Receptron`s).
///
/// Among the receptrons matching the stim: any `Abort` swallows it (returns
/// `None`); every `Amplify` scales the intensity (shields/armor - all
/// damage-reduction in SS2 data); then every `Damage` deals
/// `intensity * multiplier` (or a flat `multiplier` when `use_intensity` is
/// false; negative values heal), summed. `None` means the receiver has no
/// response to this stim at all - the Dark Engine's type-effectiveness
/// mechanism (e.g. humans have no receptron for EMP).
///
/// Amplify is applied before Damage regardless of the receptrons' `order`
/// field: in the shipped data the shield/armor Amplify receptrons carry a
/// *higher* order than the vulnerability Damage receptrons (PsiShield 78-79 vs
/// Human Vulnerability 15-36), yet a damage-reduction shield only means
/// anything if it reduces the intensity the damage receptron then reads. The
/// `order` field currently drives nothing else we chain.
pub fn resolve_stim_damage(
    receptrons: &[(i32, ReceptronOptions)],
    stim_template_id: i32,
    intensity: f32,
) -> Option<f32> {
    let mut amplify = 1.0;
    let mut has_damage = false;
    // Sum of damage multipliers, split by whether they scale with intensity, so
    // the (final) amplified intensity can be applied after all Amplifies are known.
    let mut intensity_multiplier = 0.0;
    let mut flat_damage = 0.0;

    for (_, options) in receptrons
        .iter()
        .filter(|(stim, _)| *stim == stim_template_id)
    {
        match &options.effect {
            ReceptronEffect::Abort => return None,
            ReceptronEffect::Amplify { factor } => amplify *= factor,
            ReceptronEffect::Damage {
                multiplier,
                use_intensity,
            } => {
                has_damage = true;
                if *use_intensity {
                    intensity_multiplier += multiplier;
                } else {
                    flat_damage += multiplier;
                }
            }
            ReceptronEffect::Unhandled(_) => {}
        }
    }

    has_damage.then(|| flat_damage + intensity * amplify * intensity_multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH_EXPLOSIVE: i32 = -376;
    const EMP: i32 = -374;

    fn receptron(order: i32, effect: ReceptronEffect) -> ReceptronOptions {
        ReceptronOptions { order, effect }
    }

    fn damage(order: i32, multiplier: f32) -> ReceptronOptions {
        receptron(
            order,
            ReceptronEffect::Damage {
                multiplier,
                use_intensity: true,
            },
        )
    }

    #[test]
    fn no_matching_receptron_means_no_response() {
        // A human has no EMP receptron: EMP blasts do nothing to it.
        let human = vec![(HIGH_EXPLOSIVE, damage(33, 4.0))];
        assert_eq!(resolve_stim_damage(&human, EMP, 10.0), None);
    }

    #[test]
    fn damage_scales_intensity_by_the_multiplier() {
        // Human Vulnerability vs High Explosive: x4 (shock2.gam).
        let human = vec![(HIGH_EXPLOSIVE, damage(33, 4.0))];
        assert_eq!(
            resolve_stim_damage(&human, HIGH_EXPLOSIVE, 10.0),
            Some(40.0)
        );
    }

    #[test]
    fn amplify_reduces_damage_even_at_higher_order_than_the_damage_receptron() {
        // Real data: shield/armor Amplify receptrons carry a HIGHER order than
        // the vulnerability Damage receptron (PsiShield 79 vs Human 33). The
        // shield must still reduce the damage the receptron computes.
        let shielded = vec![
            (HIGH_EXPLOSIVE, damage(33, 4.0)),
            (
                HIGH_EXPLOSIVE,
                receptron(79, ReceptronEffect::Amplify { factor: 0.5 }),
            ),
        ];
        // 10 intensity * 0.5 shield * 4.0 vulnerability = 20.
        assert_eq!(
            resolve_stim_damage(&shielded, HIGH_EXPLOSIVE, 10.0),
            Some(20.0)
        );
    }

    #[test]
    fn multiple_amplifies_multiply_together() {
        let doubly_shielded = vec![
            (
                HIGH_EXPLOSIVE,
                receptron(79, ReceptronEffect::Amplify { factor: 0.5 }),
            ),
            (
                HIGH_EXPLOSIVE,
                receptron(90, ReceptronEffect::Amplify { factor: 0.4 }),
            ),
            (HIGH_EXPLOSIVE, damage(33, 1.0)),
        ];
        // 10 * 0.5 * 0.4 * 1.0 = 2.
        assert_eq!(
            resolve_stim_damage(&doubly_shielded, HIGH_EXPLOSIVE, 10.0),
            Some(2.0)
        );
    }

    #[test]
    fn abort_swallows_the_stim() {
        // Invulnerable: Abort ahead of any damage.
        let invulnerable = vec![
            (HIGH_EXPLOSIVE, receptron(1, ReceptronEffect::Abort)),
            (HIGH_EXPLOSIVE, damage(33, 4.0)),
        ];
        assert_eq!(
            resolve_stim_damage(&invulnerable, HIGH_EXPLOSIVE, 10.0),
            None
        );
    }

    #[test]
    fn flat_damage_ignores_intensity_and_unhandled_effects_are_inert() {
        let receiver = vec![
            (
                HIGH_EXPLOSIVE,
                receptron(5, ReceptronEffect::Unhandled("EnvSound".to_string())),
            ),
            (
                HIGH_EXPLOSIVE,
                receptron(
                    10,
                    ReceptronEffect::Damage {
                        multiplier: 7.0,
                        use_intensity: false,
                    },
                ),
            ),
        ];
        assert_eq!(
            resolve_stim_damage(&receiver, HIGH_EXPLOSIVE, 100.0),
            Some(7.0)
        );
    }
}
