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

/// The entity held in `hand`, weapon or not.
pub fn held_in_hand(world: &World, hand: Handedness) -> Option<EntityId> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    match hand {
        Handedness::Left => player_info.left_hand_entity_id,
        Handedness::Right => player_info.right_hand_entity_id,
    }
}

/// Whether `entity` is held in either hand.
pub fn is_held(world: &World, entity: EntityId) -> bool {
    held_in_hand(world, Handedness::Left) == Some(entity)
        || held_in_hand(world, Handedness::Right) == Some(entity)
}

/// The weapon held in `hand`, or `None` when that hand is empty or holds
/// something with no ammo/charge of its own (a medkit, a melee weapon).
pub fn weapon_in_hand(world: &World, hand: Handedness) -> Option<EntityId> {
    let held = held_in_hand(world, hand)?;
    is_weapon(world, held).then_some(held)
}

/// The weapon the player is wielding, for actions and readouts that are not
/// tied to one hand (the Reload/CycleAmmo input actions, the ammo readout).
/// Right hand first - see the module docs for the precedence rule.
pub fn wielded_weapon(world: &World) -> Option<EntityId> {
    weapon_in_hand(world, Handedness::Right).or_else(|| weapon_in_hand(world, Handedness::Left))
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
