mod behavior;
mod chase_behavior;
mod dead_behavior;
mod idle_behavior;
mod melee_attack_behavior;
mod noop_behavior;
mod ranged_attack_behavior;
mod scripted_sequence_behavior;
mod search_behavior;
mod wander_behavior;

pub use behavior::*;
pub use chase_behavior::*;
pub use dead_behavior::*;
pub use idle_behavior::*;
pub use melee_attack_behavior::*;
pub use ranged_attack_behavior::*;
pub use scripted_sequence_behavior::*;
pub use search_behavior::*;
pub use wander_behavior::*;

use std::cell::RefCell;

use dark::SCALE_FACTOR;
use shipyard::{EntityId, World};

use crate::scripts::ai::ai_util::{chase_target_distance, has_ranged_weapon};

/// The attack behavior for the current distance to the player, or None when
/// out of attack range (the caller should chase to close the distance).
/// Single source of truth for the combat-range thresholds - used both by
/// the High-alertness behavior mapping and ChaseBehavior's transitions, so
/// the two mechanisms can't disagree.
pub fn attack_behavior_for_distance(
    world: &World,
    entity_id: EntityId,
) -> Option<Box<RefCell<dyn Behavior>>> {
    let distance = chase_target_distance(world, entity_id)?;
    let melee_attack_distance = 8.0 / SCALE_FACTOR;
    let ranged_max_attack_distance = 40.0 / SCALE_FACTOR;
    let ranged_min_attack_distance = 15.0 / SCALE_FACTOR;

    // Only ranged-armed AIs stop to shoot; melee AIs must keep chasing or
    // they stall at mid-range bouncing between chase and ranged-attack.
    if distance > ranged_min_attack_distance
        && distance < ranged_max_attack_distance
        && has_ranged_weapon(world, entity_id)
    {
        Some(Box::new(RefCell::new(RangedAttackBehavior)))
    } else if distance < melee_attack_distance {
        Some(Box::new(RefCell::new(MeleeAttackBehavior)))
    } else {
        None
    }
}
