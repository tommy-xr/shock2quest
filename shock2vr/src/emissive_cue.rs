//! Item state read off the glow of a held item's authored emissive parts
//! (the 25AE models give them their own material slot), e.g. the psi amp's
//! strip dims as psi points drain and the fusion cannon's tubes as it
//! empties; a looted crate's lights go out. Only scales emissivity the art
//! already has, so models without a glowing part are unaffected.
use dark::properties::{Link, Links, PropGunState, PropWeaponType};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    dev_params, mission::mission_core::GlobalTemplateHierarchy, runtime_props::RuntimePropSearched,
    time::Time,
};

/// Crates and lockers, whose glow reports their contents. Desks run the same
/// loot script, but their glow is a console screen, so they are left out.
const LOOT_CONTAINER_TEMPLATES: [i32; 4] = [
    -122,  // Crate #1
    -941,  // Small Trioptimum Crate
    -1303, // Locker
    -1886, // Hackable Crate (and the SuperCrate below it)
];

/// Glow left at a sliver of charge, so "low" still reads as "on".
const FLOOR: f32 = 0.25;
/// Below this fraction the glow stutters, warning of an imminent empty.
const LOW: f32 = 0.15;

/// Emissivity scale for a held item, or `None` to keep the authored glow.
pub(crate) fn held_scale(world: &World, item: EntityId) -> Option<f32> {
    if !dev_params::get_bool(dev_params::EMISSIVE_CUES) {
        return None;
    }
    let fraction = charge_fraction(world, item)?;
    let secs = world.borrow::<UniqueView<Time>>().ok()?.total.as_secs_f32();
    let flicker = dev_params::get_bool(dev_params::EMISSIVE_CUE_FLICKER);
    Some(glow(fraction, secs, flicker))
}

/// Emissivity scale for an item in the world: a searched loot container with
/// nothing left to take (emptied, or ruined by a failed hack) goes dark, so it
/// reads as searched from across the room. Unsearched ones keep their glow,
/// so it never gives away which crates are worth opening.
pub(crate) fn world_scale(world: &World, entity: EntityId) -> Option<f32> {
    if !dev_params::get_bool(dev_params::EMISSIVE_CUES) {
        return None;
    }
    let searched = world
        .borrow::<View<RuntimePropSearched>>()
        .is_ok_and(|searched| searched.contains(entity));
    if !searched || !is_loot_container(world, entity) {
        return None;
    }
    let spent = !contains_anything(world, entity)
        || crate::scripts::gui::hackable_crate::is_ruined(world, entity);
    spent.then_some(0.0)
}

fn is_loot_container(world: &World, entity: EntityId) -> bool {
    let Some(template) = crate::scripts::script_util::entity_class_template_id(world, entity)
    else {
        return false;
    };
    world
        .borrow::<UniqueView<GlobalTemplateHierarchy>>()
        .is_ok_and(|h| {
            LOOT_CONTAINER_TEMPLATES
                .iter()
                .any(|&loot| h.is_or_descends_from(template, loot))
        })
}

fn contains_anything(world: &World, container: EntityId) -> bool {
    world.borrow::<View<Links>>().is_ok_and(|links| {
        links.get(container).is_ok_and(|links| {
            links
                .to_links
                .iter()
                .any(|link| matches!(link.link, Link::Contains(_)))
        })
    })
}

/// How full `item` is, for the items whose glow reports it.
fn charge_fraction(world: &World, item: EntityId) -> Option<f32> {
    if crate::wielded_weapon::is_psi_amp(world, item) {
        // Same fraction the HUD psi bar shows.
        return Some(crate::hud::get_psi_percentage(world));
    }
    if !is_high_tech_gun(world, item) {
        return None;
    }
    let setting = crate::scripts::script_util::active_gun_setting(world, item)?;
    let ammo = world
        .borrow::<View<PropGunState>>()
        .ok()?
        .get(item)
        .ok()?
        .ammo;
    if setting.clip <= 0 {
        return None;
    }
    // Too little left for one more shot reads as empty (the laser pistol
    // spends 3 a shot, so it can stall at 2).
    if ammo < crate::scripts::weapon_script::rounds_per_shot(&setting) {
        return Some(0.0);
    }
    Some(ammo as f32 / setting.clip as f32)
}

/// Energy (laser pistol, EMP rifle) and Heavy (fusion cannon; the grenade
/// launcher and stasis gun too, but their art has no glow) weapon types.
/// Standard guns (e.g. the assault rifle) keep their glow.
fn is_high_tech_gun(world: &World, item: EntityId) -> bool {
    const ENERGY: i32 = 1;
    const HEAVY: i32 = 2;
    world
        .borrow::<View<PropWeaponType>>()
        .ok()
        .and_then(|types| types.get(item).ok().map(|t| t.0))
        .is_some_and(|t| t == ENERGY || t == HEAVY)
}

/// Quadratic above a floor - the glow saturates on top of the lit colour, so
/// linear left half charge looking full. Optionally stutters when low; dark
/// at empty.
fn glow(fraction: f32, secs: f32, flicker: bool) -> f32 {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return 0.0;
    }
    let level = FLOOR + (1.0 - FLOOR) * fraction * fraction;
    // Two incommensurate sines give an irregular, deterministic stutter.
    let dropout = (secs * 23.0).sin() + (secs * 9.1).sin() > 1.2;
    if flicker && fraction < LOW && dropout {
        0.0
    } else {
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{GunSettingDesc, PropBaseGunDesc};

    const STANDARD: i32 = 0;
    const ENERGY: i32 = 1;
    const HEAVY: i32 = 2;

    /// A gun of `weapon_type` holding `ammo` of a 20-round clip, 3 a shot.
    fn gun(weapon_type: i32, ammo: i32) -> (World, EntityId) {
        let mut world = World::new();
        let setting = GunSettingDesc {
            clip: 20,
            ammo_usage: 3,
            ..GunSettingDesc::default()
        };
        let gun = world.add_entity((
            PropWeaponType(weapon_type),
            PropGunState {
                ammo,
                condition: 100.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            PropBaseGunDesc {
                settings: [setting.clone(), setting.clone(), setting],
            },
        ));
        (world, gun)
    }

    #[test]
    fn high_tech_guns_report_their_clip_and_standard_guns_do_not() {
        let (world, energy) = gun(ENERGY, 5);
        assert_eq!(charge_fraction(&world, energy), Some(0.25));
        let (world, heavy) = gun(HEAVY, 20);
        assert_eq!(charge_fraction(&world, heavy), Some(1.0));
        // One shot's worth still fires; less reads empty.
        let (world, last_shot) = gun(ENERGY, 3);
        assert_eq!(charge_fraction(&world, last_shot), Some(0.15));
        let (world, stalled) = gun(ENERGY, 2);
        assert_eq!(charge_fraction(&world, stalled), Some(0.0));
        // Over capacity (debug scenes load 50 into a 40 clip) is clamped by `glow`.
        let (world, overfull) = gun(HEAVY, 30);
        assert_eq!(
            charge_fraction(&world, overfull).map(|f| glow(f, 0.0, false)),
            Some(1.0)
        );
        let (world, standard) = gun(STANDARD, 5);
        assert_eq!(charge_fraction(&world, standard), None);
    }

    /// An entity running `script` that holds `items` things.
    /// A searched-or-not entity of `template` holding `items` things.
    fn container(template: i32, items: usize, searched: bool) -> (World, EntityId) {
        use dark::properties::{PropTemplateId, ToLink, WrappedEntityId};
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(std::collections::HashMap::new()));
        let to_links = (0..items)
            .map(|_| ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(world.add_entity(()))),
                link: Link::Contains(0),
            })
            .collect();
        let entity = world.add_entity((
            PropTemplateId {
                template_id: template,
            },
            Links { to_links },
        ));
        if searched {
            world.add_component(entity, RuntimePropSearched);
        }
        (world, entity)
    }

    #[test]
    fn only_an_emptied_loot_container_goes_dark() {
        const LOCKER: i32 = -1303;
        const DESK: i32 = -119;
        let (world, full) = container(LOCKER, 1, true);
        assert_eq!(world_scale(&world, full), None);
        let (world, emptied) = container(LOCKER, 0, true);
        assert_eq!(world_scale(&world, emptied), Some(0.0));
        // Unsearched never gives away that it is empty.
        let (world, unsearched) = container(LOCKER, 0, false);
        assert_eq!(world_scale(&world, unsearched), None);
        // A desk's glow is its screen, not its contents.
        let (world, desk) = container(DESK, 0, true);
        assert_eq!(world_scale(&world, desk), None);
    }

    #[test]
    fn empty_is_dark_and_full_is_authored() {
        assert_eq!(glow(0.0, 0.0, true), 0.0);
        assert_eq!(glow(1.0, 0.0, true), 1.0);
    }

    #[test]
    fn brightness_rises_with_charge_above_the_floor() {
        assert!(glow(0.5, 0.0, true) > glow(0.25, 0.0, true));
        assert!(glow(0.01, 0.0, true) >= FLOOR);
    }

    #[test]
    fn only_low_charge_stutters_and_only_when_enabled() {
        let dims = |fraction, flicker| {
            (0..600)
                .any(|i| glow(fraction, i as f32 / 60.0, flicker) < glow(fraction, 0.0, flicker))
        };
        assert!(dims(0.1, true));
        assert!(!dims(0.5, true));
        assert!(!dims(0.1, false));
    }
}
