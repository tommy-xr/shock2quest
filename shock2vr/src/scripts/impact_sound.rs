//! Impact sounds for things the player holds, and the rate limiting they share.
//!
//! Two different held items arrive here by two different routes:
//!
//! - a **melee weapon** is held with a live collision group, so Rapier's narrow
//!   phase reports its touch as an ordinary contact ([`super::melee_weapon`]);
//! - a **gun** under `physical_held_items` is held *inert* - no memberships and
//!   no filter, so the projectile ray cannot hit the barrel it starts inside
//!   and nothing spawned at the muzzle can be shoved by it - and therefore
//!   generates no contact at all. What stops it at a wall is the drive's shape
//!   cast, and that cast's blocking hit is reported as the collision instead
//!   (see `PhysicsWorld::held_item_sweep_fraction`).
//!
//! Once the event exists the two are the same problem, so the guard below is
//! shared rather than reimplemented: a contact is audible only if the item was
//! closing on what it hit, and that partner then stays quiet for a moment.

use std::collections::HashMap;

use cgmath::{EuclideanSpace, InnerSpace, vec3};
use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, util::get_position_from_transform};

use super::{Effect, MessagePayload, Script, script_util::play_impact_sound};

/// How long one contact partner stays silent after an item thuds against it.
/// Rapier reports a fresh contact edge every time the solver separates and
/// re-touches, so a weapon chattering against a wall would otherwise
/// machine-gun impact sounds. Shorter than the melee free-swing damage
/// cooldown on purpose: two deliberate taps in quick succession should both be
/// audible even though only the first one is billed.
///
/// Note the key is an *entity*, and a mission's entire static geometry is one
/// collider owning one entity - so this is one thud per 0.15 s for the whole
/// level, plus one per prop. That is the right granularity for an impact
/// sound, and deliberately coarser than "per surface" would be.
pub const IMPACT_SOUND_COOLDOWN_SECONDS: f32 = 0.15;

/// Approach speed below which a contact makes no sound at all, in world units
/// per second. Only something that arrived *at* the surface clangs: an item
/// resting against one, or dragged along one, is silent.
///
/// Measured in `debug_melee` (see its module docs): a held weapon at rest
/// reads ~0.005 and a brisk controller sweep peaks at ~1.6. This sits well
/// clear of rest while staying far below the free-swing *damage* threshold
/// (0.5 in that scene), so a light tap that does no damage still clinks.
const IMPACT_SOUND_MIN_SPEED: f32 = 0.1;

/// Per-partner rate limiting for a held item's impact sounds.
#[derive(Default)]
pub struct ImpactSoundGuard {
    /// Remaining seconds before this item may thud against the same contact
    /// partner again. Deliberately independent of any damage cooldown: a wall
    /// makes a noise whether or not the contact is billable.
    cooldowns: HashMap<EntityId, f32>,
}

impl ImpactSoundGuard {
    /// Expire the per-partner cooldowns.
    pub fn tick(&mut self, elapsed: f32) {
        self.cooldowns.retain(|_, remaining| {
            *remaining -= elapsed;
            *remaining > 0.0
        });
    }

    /// Whether this contact should be heard, arming the partner's cooldown
    /// when it should.
    ///
    /// The speed is taken *along the contact normal*, not as a raw magnitude.
    /// A held item carries the player's whole locomotion velocity, so a wrench
    /// brushing a corridor wall while walking reads fast by magnitude while
    /// barely closing on the wall at all - and would otherwise clang every
    /// 0.15 s for the length of the corridor. `abs` because the normal's
    /// orientation depends on which collider was listed first.
    ///
    /// This reads the body's velocity even though a held item is a *kinematic*
    /// body, which is only sound because Rapier derives a
    /// `KinematicPositionBased` body's velocity each step from how far it
    /// actually moved. Measured on the swept drive: an item arriving at a wall
    /// reports its real closing speed on the frame it is stopped and 0.000 on
    /// every frame it spends resting there - which is exactly the distinction
    /// this guard wants, so the sweep's own numbers are not needed.
    pub fn should_play(
        &mut self,
        entity_id: EntityId,
        with: EntityId,
        physics: &PhysicsWorld,
        contact: Option<crate::physics::CollisionContact>,
    ) -> bool {
        if self.cooldowns.contains_key(&with) {
            return false;
        }
        let velocity = physics
            .get_velocity(entity_id)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
        let speed = match contact {
            Some(contact) => velocity.dot(contact.normal).abs(),
            None => velocity.magnitude(),
        };
        if speed < IMPACT_SOUND_MIN_SPEED {
            return false;
        }
        self.arm(with);
        true
    }

    /// Silence this partner for the cooldown without asking whether the sound
    /// should play - for a caller that decided to play it on other grounds.
    pub fn arm(&mut self, with: EntityId) {
        self.cooldowns.insert(with, IMPACT_SOUND_COOLDOWN_SECONDS);
    }
}

/// Impact sound for a contact: the item's collision schema (its class tag +
/// the hit material - wrench on metal clangs, on a creature thuds), unless its
/// collision type opts out. Needs no damage value: unmaterialed surfaces
/// (world geometry, a bench) fall back to the default material tag.
pub fn impact_sound_effect(entity_id: EntityId, with: EntityId, world: &World) -> Effect {
    let no_sound = world
        .borrow::<View<PropCollisionType>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|c| c.collision_type))
        .is_some_and(|flags| flags.contains(CollisionType::NO_COLLISION_SOUND));
    if no_sound {
        return Effect::NoEffect;
    }
    let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
    play_impact_sound(world, entity_id, with, position.to_vec())
}

/// The audible half of a physically-held item that is *not* a melee weapon -
/// today, a gun under `physical_held_items`.
///
/// It deliberately does nothing else. A gun is not a club: the contact carries
/// no damage, opens no attack window, and the item is inert to the solver, so
/// the only thing a block against the level should produce is the noise of it.
///
/// The guard on `is_held_inert` is what keeps that promise narrow. The only
/// contacts an inert held item can receive are the blocks its own sweep found,
/// so this script is silent for the same gun lying on the floor, thrown, or
/// wielded flat - none of which is what the sweep is about, and all of which
/// would otherwise be a behavior change nobody asked for.
pub struct HeldItemImpactSound {
    guard: ImpactSoundGuard,
}

impl HeldItemImpactSound {
    pub fn new() -> Self {
        Self {
            guard: ImpactSoundGuard::default(),
        }
    }
}

impl Script for HeldItemImpactSound {
    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &crate::time::Time,
    ) -> Effect {
        self.guard.tick(time.elapsed.as_secs_f32());
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::Collided { with, contact } = msg else {
            return Effect::NoEffect;
        };
        if !physics.is_held_inert(entity_id) {
            return Effect::NoEffect;
        }
        if !self.guard.should_play(entity_id, *with, physics, *contact) {
            return Effect::NoEffect;
        }
        impact_sound_effect(entity_id, *with, world)
    }
}

#[cfg(test)]
mod tests {
    use cgmath::Quaternion;
    use dark::properties::PropClassTag;
    use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape, Vector};
    use shipyard::World;

    use crate::physics::{
        CollisionEvent, CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld,
    };

    use super::*;

    /// Run the real drive: a gun held inert at x=-1, a fixed wall at x=0, and
    /// the hand asking for x=+1. Returns the physics world plus the `Collided`
    /// payloads the mission loop would dispatch to the gun - so the script is
    /// tested against the events the game actually produces, not a hand-built
    /// contact.
    fn gun_pushed_into_a_wall(gun: EntityId) -> (PhysicsWorld, Vec<MessagePayload>) {
        let mut physics = PhysicsWorld::new();
        let floor = physics.create_static_body(
            Isometry::translation(0.0, -1.0, 0.0),
            EntityId::from_inner(1000),
        );
        physics.attach_collider(
            floor,
            SharedShape::cuboid(100.0, 1.0, 100.0),
            1.0,
            CollisionGroup::entity(),
        );
        let mut player = physics.create_player(
            vec3(1000.0, 1000.0, 1000.0),
            EntityId::from_inner(1001).unwrap(),
        );
        physics.add_collider(
            EntityId::from_inner(2).unwrap(),
            ColliderBuilder::cuboid(0.05, 1.0, 1.0)
                .translation(Vector::new(0.0, 1.0, 0.0))
                .build(),
        );

        physics.add_dynamic(
            gun,
            vec3(-1.0, 1.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.4, 0.4, 0.4)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        physics.set_held_item_physical(gun, CollisionGroup::held_inert());
        physics.set_position_rotation2(
            gun,
            vec3(-1.0, 1.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        physics.set_position_rotation2(
            gun,
            vec3(1.0, 1.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );

        // Stops on the frame that produced the block, so the physics world
        // handed back is in the state the mission loop would dispatch against:
        // a held item's velocity is the distance it actually moved that step,
        // and it is 0 again a frame later.
        let mut messages = Vec::new();
        for frame in 0.. {
            assert!(frame < 120, "the drive never reported blocking on the wall");
            let (_, events) = physics.update(vec3(0.0, 0.0, 0.0), &mut player);
            for event in events {
                // The same translation `mission_core` performs: whichever side
                // the gun is, it hears about the other, with the normal
                // oriented toward it.
                if let CollisionEvent::CollisionStarted {
                    entity1_id,
                    entity2_id,
                    contact,
                } = event
                {
                    if entity2_id == gun {
                        messages.push(MessagePayload::Collided {
                            with: entity1_id,
                            contact: contact.map(|contact| crate::physics::CollisionContact {
                                point: contact.point,
                                normal: -contact.normal,
                            }),
                        });
                    } else if entity1_id == gun {
                        messages.push(MessagePayload::Collided {
                            with: entity2_id,
                            contact,
                        });
                    }
                }
            }
            if !messages.is_empty() {
                break;
            }
        }
        (physics, messages)
    }

    /// A world holding one gun that resolves an impact sound: the schema
    /// lookup is keyed on the item's class tag.
    fn world_with_audible_gun() -> (World, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity(PropClassTag::from_string("WeaponType Pistol"));
        (world, gun)
    }

    fn sound_count(effect: &Effect) -> usize {
        match effect {
            Effect::PlayEnvironmentalSound { .. } => 1,
            Effect::Multiple(effects) => effects.iter().map(sound_count).sum(),
            _ => 0,
        }
    }

    /// The report: a gun pushed into a bulkhead stopped dead and in silence,
    /// because an inert held item generates no contact for anything to hear.
    #[test]
    fn a_held_gun_blocked_by_the_level_makes_one_impact_sound() {
        let (world, gun) = world_with_audible_gun();
        let (physics, messages) = gun_pushed_into_a_wall(gun);
        let mut script = HeldItemImpactSound::new();

        assert_eq!(
            messages.len(),
            1,
            "the drive should report the block exactly once: {messages:?}"
        );
        let effect = script.handle_message(gun, &world, &physics, &messages[0]);
        assert_eq!(sound_count(&effect), 1, "got {effect:?}");
    }

    /// ...and it does not machine-gun. The block itself is edge-triggered, but
    /// a gun chattering against a surface would still re-report; the same
    /// partner stays quiet until the cooldown expires.
    #[test]
    fn a_repeat_block_within_the_cooldown_is_silent() {
        let (world, gun) = world_with_audible_gun();
        let (physics, messages) = gun_pushed_into_a_wall(gun);
        let mut script = HeldItemImpactSound::new();

        let first = script.handle_message(gun, &world, &physics, &messages[0]);
        assert_eq!(sound_count(&first), 1, "got {first:?}");
        for _ in 0..5 {
            let repeat = script.handle_message(gun, &world, &physics, &messages[0]);
            assert_eq!(sound_count(&repeat), 0, "got {repeat:?}");
        }

        script.update(
            gun,
            &world,
            &physics,
            &crate::time::Time {
                elapsed: std::time::Duration::from_millis(200),
                total: std::time::Duration::from_millis(200),
            },
        );
        let later = script.handle_message(gun, &world, &physics, &messages[0]);
        assert_eq!(sound_count(&later), 1, "got {later:?}");
    }

    /// The script's whole remit is the swept, inert, held state. The same gun
    /// lying on the floor collides for ordinary physical reasons, and turning
    /// those into noise is a different feature nobody asked for.
    #[test]
    fn a_gun_that_is_not_held_inert_stays_silent() {
        let (world, gun) = world_with_audible_gun();
        let (mut physics, messages) = gun_pushed_into_a_wall(gun);
        physics.set_collision_group(gun, CollisionGroup::entity());
        physics.set_held_item_physical(gun, CollisionGroup::entity());
        let mut script = HeldItemImpactSound::new();

        assert!(matches!(
            script.handle_message(gun, &world, &physics, &messages[0]),
            Effect::NoEffect
        ));
    }
}
