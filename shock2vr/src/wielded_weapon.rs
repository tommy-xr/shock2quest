//! Which weapon a weapon action or an ammo readout means.
//!
//! `PlayerInfo` carries two hand slots. In flatscreen the controller wields
//! into the *left* slot and the right is always empty (see
//! `FlatInteraction::held_entities`), so "the left-hand entity" happened to be
//! a correct spelling of "the wielded weapon". In VR the slots are literally
//! the two motion controllers, so a gun held in the right hand was invisible
//! to reload, ammo cycling, and the ammo HUD - all of which read the left slot
//! only. This module is the single place that answers the question, for both
//! presentations.
//!
//! **Both-hands precedence.** When both hands hold a weapon, the RIGHT hand
//! wins. It is the dominant/aiming hand for the default VR player, it is the
//! hand the forearm ammo panel is worn beside, and - because flat never fills
//! the right slot - preferring it leaves flatscreen behaviour exactly as it
//! was. A future hand-specific affordance (e.g. a reload gesture that starts
//! from one controller) should resolve its own hand with [`weapon_in_hand`]
//! rather than going through [`wielded_weapon`].

use dark::properties::PropGunState;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, vr_config::Handedness};

/// The weapon held in `hand`, or `None` when that hand is empty or holds
/// something with no ammo/charge of its own (a medkit, a melee weapon).
pub fn weapon_in_hand(world: &World, hand: Handedness) -> Option<EntityId> {
    let held = held_by_hand(world, hand)?;
    is_weapon(world, held).then_some(held)
}

/// Whatever `hand` holds, weapon or not. The one place `PlayerInfo`'s two
/// slots are read by handedness.
pub fn held_by_hand(world: &World, hand: Handedness) -> Option<EntityId> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    match hand {
        Handedness::Left => player_info.left_hand_entity_id,
        Handedness::Right => player_info.right_hand_entity_id,
    }
}

/// Whether the player holds `entity` in either hand. Unlike
/// [`weapon_in_hand`] this asks nothing about what the object is, so it also
/// covers a gun with no clip of its own. `PlayerInfo` is written before scripts
/// run each frame, so a pickup is visible to the same frame's scripts.
pub fn held_in_hand(world: &World, entity: EntityId) -> bool {
    world
        .borrow::<UniqueView<PlayerInfo>>()
        .map(|info| {
            info.left_hand_entity_id == Some(entity) || info.right_hand_entity_id == Some(entity)
        })
        .unwrap_or(false)
}

/// The weapon the player is wielding, for actions and readouts that are not
/// tied to one hand (the Reload/CycleAmmo input actions, the ammo readout).
/// Right hand first - see the module docs for the precedence rule.
pub fn wielded_weapon(world: &World) -> Option<EntityId> {
    weapon_in_hand(world, Handedness::Right).or_else(|| weapon_in_hand(world, Handedness::Left))
}

/// The wielded psi amp, in either hand - what the psi HUD readout reports and
/// what the power selection MFD presents.
pub fn wielded_psi_amp(world: &World) -> Option<EntityId> {
    wielded_weapon(world).filter(|weapon| is_psi_amp(world, *weapon))
}

/// The weapon a per-hand gun action (eject, fire-mode toggle) applies to.
/// `Some(hand)` - a face button - means the weapon in THAT hand, so a
/// dual-wielding player acts on the gun they pressed; `None` - the
/// hand-agnostic input action - means whichever weapon is wielded. Both arms
/// filter to weapons, so a hand carrying a medkit is not a target.
pub fn hand_weapon_target(world: &World, hand: Option<Handedness>) -> Option<EntityId> {
    match hand {
        Some(hand) => weapon_in_hand(world, hand),
        None => wielded_weapon(world),
    }
}

/// Whether the held `entity` is a weapon the ammo readout and the reload /
/// ammo-cycle actions apply to: a gun (it has a `PropGunState` clip) or the
/// psi amp (whose readout is its selected power, not a clip).
fn is_weapon(world: &World, entity: EntityId) -> bool {
    if world
        .borrow::<View<PropGunState>>()
        .map(|v| v.get(entity).is_ok())
        .unwrap_or(false)
    {
        return true;
    }
    is_psi_amp(world, entity)
}

/// Whether `entity` is an energy weapon - one that recharges instead of taking
/// clips, so it offers no reload control. Identified by the `EnergyWeapon`
/// script the `Energy` archetype hands down, which is the thing that makes it
/// rechargeable.
pub(crate) fn is_energy_weapon(world: &World, entity: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, entity, "energyweapon")
}

/// Whether `entity`'s template carries the `weapontype psiamp` class tag.
pub(crate) fn is_psi_amp(world: &World, entity: EntityId) -> bool {
    let Some(template_id) = crate::scripts::script_util::entity_class_template_id(world, entity)
    else {
        return false;
    };
    world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateClassTags>>()
        .ok()
        .and_then(|tags| tags.0.get(&template_id)?.get("weapontype").cloned())
        .is_some_and(|weapon_type| weapon_type == "psiamp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Quaternion, Vector3};

    /// A world whose player holds a GUN in each named hand, returned with the
    /// two held ids.
    fn player_holding(left: bool, right: bool) -> (World, Option<EntityId>, Option<EntityId>) {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        let mut gun = |world: &mut World| {
            world.add_entity((PropGunState {
                ammo: 12,
                condition: 1.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },))
        };
        let left_hand_entity_id = left.then(|| gun(&mut world));
        let right_hand_entity_id = right.then(|| gun(&mut world));
        world.add_unique(PlayerInfo {
            pos: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id,
            right_hand_entity_id,
            inventory_entity_id: inventory,
        });
        (world, left_hand_entity_id, right_hand_entity_id)
    }

    /// A per-hand action means the gun in that hand and no other - the whole
    /// point of carrying the hand through, since dual wielding otherwise hits
    /// whichever gun `wielded_weapon` happens to prefer.
    #[test]
    fn a_per_hand_action_targets_only_that_hand() {
        let (world, left_gun, right_gun) = player_holding(true, true);

        assert_eq!(
            hand_weapon_target(&world, Some(Handedness::Right)),
            right_gun
        );
        assert_eq!(hand_weapon_target(&world, Some(Handedness::Left)), left_gun);
        assert_ne!(left_gun, right_gun);
    }

    /// An empty hand ejects nothing, even with a gun in the other one.
    #[test]
    fn an_empty_hand_has_nothing_to_eject() {
        let (world, _, _) = player_holding(false, true);

        assert_eq!(hand_weapon_target(&world, Some(Handedness::Left)), None);
    }

    /// A hand carrying something that is not a weapon is not an eject target
    /// either - both arms filter the same way.
    #[test]
    fn a_hand_holding_something_that_is_not_a_weapon_has_nothing_to_eject() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        let medkit = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: Some(medkit),
            inventory_entity_id: inventory,
        });

        assert_eq!(hand_weapon_target(&world, Some(Handedness::Right)), None);
    }
}
