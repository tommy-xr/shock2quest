mod behavior;
mod chase_behavior;
mod dead_behavior;
mod idle_behavior;
mod melee_attack_behavior;
mod noop_behavior;
mod patrol_behavior;
mod ranged_attack_behavior;
mod scripted_sequence_behavior;
mod search_behavior;
mod self_destruct_behavior;
mod wander_behavior;

pub use behavior::*;
pub use chase_behavior::*;
pub use dead_behavior::*;
pub use idle_behavior::*;
pub use melee_attack_behavior::*;
pub use patrol_behavior::*;
pub use ranged_attack_behavior::*;
pub use scripted_sequence_behavior::*;
pub use search_behavior::*;
pub use self_destruct_behavior::*;
pub use wander_behavior::*;

use std::cell::RefCell;

use dark::SCALE_FACTOR;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::ai::ai_util::{
        chase_target, chase_target_distance, has_line_of_fire, has_ranged_weapon,
        is_self_destructing,
    },
};

/// How close a protocol droid gets before lighting its fuse. Same reach as a
/// melee swing - the detonation IS its melee attack (see
/// `SelfDestructBehavior`) - but named separately so tuning the blast's
/// trigger doesn't move every creature's melee range.
pub const PROTOCOL_DETONATION_RANGE: f32 = crate::scripts::ai::ai_util::MELEE_ATTACK_RANGE;

/// The attack behavior for the current distance to the player, or None when
/// out of attack range (the caller should chase to close the distance).
/// Single source of truth for the combat-range thresholds - used both by
/// the High-alertness behavior mapping and ChaseBehavior's transitions, so
/// the two mechanisms can't disagree.
pub fn attack_behavior_for_distance(
    world: &World,
    physics: &PhysicsWorld,
    entity_id: EntityId,
) -> Option<Box<RefCell<dyn Behavior>>> {
    // Distance and line of fire both gate against the AI's KNOWN target
    // (the last-known position when awareness is published, the player's
    // true position otherwise) - the same point chase steering faces and
    // the FIRE flag shoots toward, so the ray can't approve a shot the AI
    // isn't actually taking.
    let target = chase_target(world, entity_id)?;
    let distance = chase_target_distance(world, entity_id)?;
    let melee_attack_distance = 8.0 / SCALE_FACTOR;
    let ranged_max_attack_distance = 40.0 / SCALE_FACTOR;
    let ranged_min_attack_distance = 15.0 / SCALE_FACTOR;

    // A protocol droid has no weapon to swing: closing the distance starts
    // its self-destruct instead of a melee attack.
    if distance < PROTOCOL_DETONATION_RANGE && is_self_destructing(world, entity_id) {
        return Some(Box::new(RefCell::new(SelfDestructBehavior::new())));
    }

    // Only ranged-armed AIs stop to shoot; melee AIs must keep chasing or
    // they stall at mid-range bouncing between chase and ranged-attack.
    // Stopping also requires an actual line of fire: a target that is
    // KNOWN but occluded (heard through a wall, straight-line close on
    // another floor) must be chased, or the AI stands rooted firing into
    // geometry for as long as its alertness holds - permanently under a
    // pinned alert (issue #481's stand-and-shoot freeze).
    if distance > ranged_min_attack_distance
        && distance < ranged_max_attack_distance
        && has_ranged_weapon(world, entity_id)
        && has_line_of_fire(entity_id, world, physics, target)
    {
        Some(Box::new(RefCell::new(RangedAttackBehavior)))
    } else if distance < melee_attack_distance {
        Some(Box::new(RefCell::new(MeleeAttackBehavior)))
    } else {
        None
    }
}
