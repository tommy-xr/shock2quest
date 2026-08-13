use std::collections::HashSet;

use cgmath::{EuclideanSpace, vec3};
use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    PresentationMode, mission::GlobalPresentationMode, physics::PhysicsWorld,
    util::get_position_from_transform,
};

use super::{Effect, Message, MessagePayload, Script, script_util::play_impact_sound};

// Script to handle collision type
pub struct MeleeWeapon {}

impl MeleeWeapon {
    pub fn new() -> MeleeWeapon {
        MeleeWeapon {}
    }
}

/// Contact damage for the player's authored melee weapons (`PropLimbModel`).
///
/// Dark opens melee collision only during an attack window. In VR the trigger
/// edge is that production attack gesture: contacts before it, after release,
/// and after dropping the weapon are harmless. A target can be damaged only
/// once per pull even if the physical swing chatters across several contacts.
///
/// Physical contact is *never* the damage trigger outside VR - a flat swing is
/// `WeaponScript`'s aimed short-range raycast, so bumping a wielded wrench into
/// scenery must do nothing. The whole script is therefore inert in flat mode
/// rather than guarding individual messages.
pub struct TriggeredMeleeWeapon {
    attack_active: bool,
    hit_entities: HashSet<EntityId>,
}

impl TriggeredMeleeWeapon {
    pub fn new() -> Self {
        Self {
            attack_active: false,
            hit_entities: HashSet::new(),
        }
    }
}

impl Script for TriggeredMeleeWeapon {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !is_vr(world) {
            return Effect::NoEffect;
        }

        match msg {
            MessagePayload::TriggerPull => {
                self.attack_active = true;
                self.hit_entities.clear();
                Effect::NoEffect
            }
            MessagePayload::TriggerRelease | MessagePayload::Drop => {
                self.attack_active = false;
                self.hit_entities.clear();
                Effect::NoEffect
            }
            MessagePayload::Collided { with }
                if self.attack_active && self.hit_entities.insert(*with) =>
            {
                melee_impact(entity_id, *with, world)
            }
            _ => Effect::NoEffect,
        }
    }
}

fn is_vr(world: &World) -> bool {
    world
        .borrow::<UniqueView<GlobalPresentationMode>>()
        .map(|mode| mode.0 == PresentationMode::Vr)
        .unwrap_or(false)
}

fn melee_impact(entity_id: EntityId, with: EntityId, world: &World) -> Effect {
    let damage_effect = Effect::Send {
        msg: Message {
            to: with,
            payload: MessagePayload::Damage {
                amount: 1.0,
                impact: None,
            },
        },
    };
    // Impact sound: the weapon's collision schema (weapontype class tag + hit
    // material - wrench on metal clangs, on a creature thuds), unless its
    // collision type opts out.
    let no_sound = world
        .borrow::<View<PropCollisionType>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|c| c.collision_type))
        .is_some_and(|flags| flags.contains(CollisionType::NO_COLLISION_SOUND));
    let sound_effect = if no_sound {
        Effect::NoEffect
    } else {
        let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
        play_impact_sound(world, entity_id, with, position.to_vec())
    };
    Effect::Multiple(vec![damage_effect, sound_effect])
}

impl Script for MeleeWeapon {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with } => melee_impact(entity_id, *with, world),
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world(mode: PresentationMode) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        world.add_unique(GlobalPresentationMode(mode));
        let weapon = world.add_entity(PropCollisionType {
            collision_type: CollisionType::NO_COLLISION_SOUND,
        });
        let target = world.add_entity(());
        (world, weapon, target)
    }

    fn assert_damage(effect: Effect, target: EntityId) {
        let Effect::Multiple(effects) = effect else {
            panic!("expected melee impact effects, got {effect:?}");
        };
        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == target
                        && matches!(
                            msg.payload,
                            MessagePayload::Damage { amount, impact: None }
                                if (amount - 1.0).abs() < f32::EPSILON
                        )
            )
        }));
    }

    fn collide(
        script: &mut TriggeredMeleeWeapon,
        world: &World,
        weapon: EntityId,
        target: EntityId,
    ) -> Effect {
        script.handle_message(
            weapon,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Collided { with: target },
        )
    }

    #[test]
    fn authored_melee_contact_is_harmless_until_vr_trigger_pull() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );
        assert_damage(collide(&mut script, &world, weapon, target), target);
    }

    #[test]
    fn one_vr_trigger_pull_can_damage_each_contact_only_once() {
        let (world, weapon, target) = test_world(PresentationMode::Vr);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert_damage(collide(&mut script, &world, weapon, target), target);
        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }

    #[test]
    fn release_and_drop_close_the_vr_melee_damage_window() {
        for close in [MessagePayload::TriggerRelease, MessagePayload::Drop] {
            let (world, weapon, target) = test_world(PresentationMode::Vr);
            let mut script = TriggeredMeleeWeapon::new();
            script.handle_message(
                weapon,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::TriggerPull,
            );
            script.handle_message(weapon, &world, &PhysicsWorld::new(), &close);

            assert!(matches!(
                collide(&mut script, &world, weapon, target),
                Effect::NoEffect
            ));
        }
    }

    #[test]
    fn flat_trigger_does_not_arm_physical_melee_damage() {
        let (world, weapon, target) = test_world(PresentationMode::Flat);
        let mut script = TriggeredMeleeWeapon::new();
        script.handle_message(
            weapon,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TriggerPull,
        );

        assert!(matches!(
            collide(&mut script, &world, weapon, target),
            Effect::NoEffect
        ));
    }
}
