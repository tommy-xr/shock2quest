mod back_off_behavior;
mod behavior;
mod chase_behavior;
mod dead_behavior;
mod frustration_behavior;
mod idle_behavior;
mod melee_attack_behavior;
mod noop_behavior;
mod patrol_behavior;
mod ranged_attack_behavior;
mod scripted_sequence_behavior;
mod search_behavior;
mod self_destruct_behavior;
mod wander_behavior;

pub use back_off_behavior::BackOffBehavior;
pub use behavior::*;
pub use chase_behavior::*;
pub use dead_behavior::*;
pub use frustration_behavior::*;
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
        is_self_destructing, melee_weapon_template,
    },
};

/// How close a protocol droid gets before lighting its fuse. Same reach as a
/// melee swing - the detonation IS its melee attack (see
/// `SelfDestructBehavior`) - but named separately so tuning the blast's
/// trigger doesn't move every creature's melee range.
pub(super) const RANGED_MAX_ATTACK_DISTANCE: f32 = 40.0 / SCALE_FACTOR;

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
    attack_behavior_with_modes(world, physics, entity_id, true, true)
}

pub fn attack_behavior_with_modes(
    world: &World,
    physics: &PhysicsWorld,
    entity_id: EntityId,
    allow_melee: bool,
    allow_ranged: bool,
) -> Option<Box<RefCell<dyn Behavior>>> {
    // Distance and line of fire both gate against the AI's KNOWN target
    // (the last-known position when awareness is published, the player's
    // true position otherwise) - the same point chase steering faces and
    // the FIRE flag shoots toward, so the ray can't approve a shot the AI
    // isn't actually taking.
    let target = chase_target(world, entity_id)?;
    let distance = chase_target_distance(world, entity_id)?;
    let melee_attack_distance = 8.0 / SCALE_FACTOR;

    // Melee damage is the swing weapon's contact stims, so an AI with no
    // Weapon link cannot land a blow at all. The shotgun and grenade
    // hybrids are authored that way - an AIProjectile link and nothing to
    // swing - while a creature meant to do both, the monkeys, carries both
    // links. (A weapon whose archetype stims for no damage still swings;
    // this asks only whether there is a weapon at all.)
    let can_melee = melee_weapon_template(world, entity_id).is_some();

    // ...but only a creature that can shoot INSTEAD gives up its swing. A
    // creature with neither link - a swarm, which hurts the player through
    // a proximity stim rather than a blow - keeps closing and attacking, or
    // it would shove silently with a walk cycle playing.
    let gives_up_melee = !can_melee && has_ranged_weapon(world, entity_id);

    // A gun AI with nothing to swing keeps shooting all the way in to
    // contact rather than standing off, so being cornered by one hurts:
    // retail hybrids fire point-blank instead of closing to swing.
    let ranged_min_attack_distance = if gives_up_melee || !allow_melee {
        0.0
    } else {
        15.0 / SCALE_FACTOR
    };

    // A protocol droid has no weapon to swing: closing the distance starts
    // its self-destruct instead of a melee attack.
    if distance < PROTOCOL_DETONATION_RANGE && is_self_destructing(world, entity_id) {
        return Some(Box::new(RefCell::new(SelfDestructBehavior::new())));
    }

    // Standing distance is a movement preference, never a firing prohibition:
    // a cornered gun-only creature must still shoot when it cannot retreat.
    if allow_ranged
        && (gives_up_melee || !allow_melee || distance >= melee_attack_distance)
        && has_ranged_weapon(world, entity_id)
        && has_line_of_fire(entity_id, world, physics, target)
    {
        if let Some(stand_off) = back_off_behavior::authored_stand_off(world, entity_id) {
            if distance < stand_off.trigger
                && back_off_behavior::can_back_off(world, physics, entity_id)
            {
                return Some(Box::new(RefCell::new(BackOffBehavior::new(stand_off))));
            }
        }
    }

    // Only ranged-armed AIs stop to shoot; melee AIs must keep chasing or
    // they stall at mid-range bouncing between chase and ranged-attack.
    // Stopping also requires an actual line of fire: a target that is
    // KNOWN but occluded (heard through a wall, straight-line close on
    // another floor) must be chased, or the AI stands rooted firing into
    // geometry for as long as its alertness holds - permanently under a
    // pinned alert (issue #481's stand-and-shoot freeze).
    if allow_ranged
        && distance > ranged_min_attack_distance
        && distance < RANGED_MAX_ATTACK_DISTANCE
        && has_ranged_weapon(world, entity_id)
        && has_line_of_fire(entity_id, world, physics, target)
    {
        Some(Box::new(RefCell::new(RangedAttackBehavior)))
    } else if allow_melee && distance < melee_attack_distance && !gives_up_melee {
        Some(Box::new(RefCell::new(MeleeAttackBehavior)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Quaternion, Rotation3, vec3};
    use dark::properties::{Link, Links, PropHitPoints, PropPosition, ToLink};

    use crate::mission::PlayerInfo;

    /// An AI at the origin with the given weapon links, and the player
    /// `distance` units down +Z. The physics world is empty, so the line of
    /// fire is always clear.
    fn world_with_armed_ai(links: Vec<Link>, distance: f32) -> (World, EntityId) {
        let mut world = World::new();
        let entity_id = world.add_entity((
            PropHitPoints { hit_points: 12 },
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                rotation: Quaternion::from_angle_y(cgmath::Deg(0.0)),
                cell: 0,
            },
            Links {
                to_links: links
                    .into_iter()
                    .map(|link| ToLink {
                        to_template_id: -1,
                        to_entity_id: None,
                        link,
                    })
                    .collect(),
            },
        ));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, distance),
            rotation: Quaternion::from_angle_y(cgmath::Deg(0.0)),
            entity_id: EntityId::dead(),
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: EntityId::dead(),
        });
        (world, entity_id)
    }

    fn behavior_name(links: Vec<Link>, distance: f32) -> Option<String> {
        let (world, entity_id) = world_with_armed_ai(links, distance);
        let physics = PhysicsWorld::new();
        attack_behavior_for_distance(&world, &physics, entity_id)
            .map(|behavior| behavior.borrow().name().to_owned())
    }

    fn ai_projectile() -> Link {
        Link::AIProjectile(dark::properties::AIProjectileOptions {
            targeting_method: dark::properties::AITargetMethod::StraightLine,
            delay: 0.0,
            should_lead_target: false,
            ammo: 0,
            accuracy: 0,
            select_time: 0.0,
            joint: 0,
            vhot: 0,
        })
    }

    /// A hybrid with a pipe swings, and its swing is what carries the
    /// contact stims that damage the player.
    #[test]
    fn a_melee_armed_ai_at_contact_range_attacks_in_melee() {
        assert_eq!(
            behavior_name(vec![Link::Weapon], 1.0).as_deref(),
            Some("MeleeAttack")
        );
    }

    /// A shotgun hybrid has no `Weapon` link to resolve a blow with, so it
    /// must not mime a harmless swing - it shoots at contact range instead.
    #[test]
    fn a_gun_ai_with_no_melee_weapon_shoots_at_contact_range() {
        assert_eq!(
            behavior_name(vec![ai_projectile()], 1.0).as_deref(),
            Some("RangedAttack")
        );
    }

    /// A monkey authors both links: at contact range the melee attack still
    /// wins, so adding the close-range fire path doesn't disarm its claw.
    #[test]
    fn a_dual_armed_ai_still_melees_at_contact_range() {
        assert_eq!(
            behavior_name(vec![Link::Weapon, ai_projectile()], 1.0).as_deref(),
            Some("MeleeAttack")
        );
    }

    /// The stand-off distance is unchanged for everything that can melee.
    #[test]
    fn a_dual_armed_ai_shoots_from_stand_off_range() {
        assert_eq!(
            behavior_name(vec![Link::Weapon, ai_projectile()], 8.0).as_deref(),
            Some("RangedAttack")
        );
    }

    /// Out past its maximum range, a gun AI chases rather than firing.
    #[test]
    fn a_gun_ai_out_of_range_has_no_attack() {
        assert_eq!(behavior_name(vec![ai_projectile()], 20.0), None);
    }

    /// Only a creature that can shoot instead gives up its swing. A swarm
    /// carries neither link - it hurts the player through a proximity stim -
    /// so it must keep attacking rather than shoving with a walk cycle.
    #[test]
    fn an_ai_with_no_weapon_at_all_still_attacks_at_contact_range() {
        assert_eq!(behavior_name(vec![], 1.0).as_deref(), Some("MeleeAttack"));
    }

    /// A protocol droid deliberately has no `Weapon` link - closing the
    /// distance lights its fuse, and the melee gate must not intercept that.
    #[test]
    fn a_protocol_droid_still_self_destructs_at_contact_range() {
        let (mut world, entity_id) = world_with_armed_ai(vec![], 1.0);
        world.add_component(entity_id, dark::properties::PropAI("protocol".to_owned()));
        let physics = PhysicsWorld::new();
        let name = attack_behavior_for_distance(&world, &physics, entity_id)
            .map(|behavior| behavior.borrow().name().to_owned());
        assert_eq!(name.as_deref(), Some("SelfDestruct"));
    }
    #[test]
    fn ranged_clip_completion_reselects_the_available_attack() {
        for (links, distance, expected) in [
            (vec![ai_projectile()], 1.0, "RangedAttack"),
            (vec![ai_projectile()], 8.0, "RangedAttack"),
            (vec![ai_projectile()], 20.0, "Chase"),
            (vec![Link::Weapon, ai_projectile()], 1.0, "MeleeAttack"),
        ] {
            let (world, entity) = world_with_armed_ai(links, distance);
            let physics = PhysicsWorld::new();
            let NextBehavior::Next(next) =
                RangedAttackBehavior.next_behavior(&world, &physics, entity)
            else {
                panic!("completion must re-evaluate attack range and line of fire");
            };
            assert_eq!(next.borrow().name(), expected);
        }
    }

    #[test]
    fn ranged_clip_completion_chases_when_cover_blocks_the_shot() {
        let (mut world, entity) = world_with_armed_ai(vec![ai_projectile()], 8.0);
        let mut physics = PhysicsWorld::new();
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 4.0),
            Quaternion::from_angle_y(cgmath::Deg(0.0)),
            vec3(0.0, 0.0, 0.0),
            vec3(10.0, 10.0, 0.5),
            crate::physics::CollisionGroup::entity(),
            false,
        );
        let mut player = physics.create_player(vec3(20.0, 20.0, 20.0), EntityId::dead());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let NextBehavior::Next(next) = RangedAttackBehavior.next_behavior(&world, &physics, entity)
        else {
            panic!("blocked fire must select chase");
        };
        assert_eq!(next.borrow().name(), "Chase");
    }

    #[test]
    fn frustrated_melee_can_hand_off_to_a_real_ranged_weapon_at_contact() {
        let (world, entity) = world_with_armed_ai(vec![Link::Weapon, ai_projectile()], 1.0);
        let physics = PhysicsWorld::new();
        let attack = attack_behavior_with_modes(&world, &physics, entity, false, true).unwrap();
        assert_eq!(attack.borrow().name(), "RangedAttack");
        assert!(attack_behavior_with_modes(&world, &physics, entity, false, false).is_none());
        let attack = attack_behavior_with_modes(&world, &physics, entity, true, false).unwrap();
        assert_eq!(attack.borrow().name(), "MeleeAttack");
    }
}
