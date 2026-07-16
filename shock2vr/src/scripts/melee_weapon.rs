use cgmath::{EuclideanSpace, vec3};
use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, util::get_position_from_transform};

use super::{Effect, Message, MessagePayload, Script, script_util::play_impact_sound};

// Script to handle collision type
pub struct MeleeWeapon {}

impl MeleeWeapon {
    pub fn new() -> MeleeWeapon {
        MeleeWeapon {}
    }
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
            MessagePayload::Collided { with } => {
                let damage_effect = Effect::Send {
                    msg: Message {
                        to: *with,
                        payload: MessagePayload::Damage {
                            amount: 1.0,
                            impact: None,
                        },
                    },
                };
                // Impact sound: the weapon's collision schema (weapontype
                // class tag + hit material - wrench on metal clangs, on a
                // creature thuds), unless its collision type opts out.
                let no_sound = world
                    .borrow::<View<PropCollisionType>>()
                    .ok()
                    .and_then(|v| v.get(entity_id).ok().map(|c| c.collision_type))
                    .is_some_and(|flags| flags.contains(CollisionType::NO_COLLISION_SOUND));
                let sound_effect = if no_sound {
                    Effect::NoEffect
                } else {
                    let position =
                        get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
                    play_impact_sound(world, entity_id, *with, position.to_vec())
                };
                Effect::Multiple(vec![damage_effect, sound_effect])
            }
            _ => Effect::NoEffect,
        }
    }
}
