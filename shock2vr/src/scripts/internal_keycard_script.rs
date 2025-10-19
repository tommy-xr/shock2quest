use dark::properties::PropKeySrc;

use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct KeyCardScript {}
impl KeyCardScript {
    pub fn new() -> KeyCardScript {
        KeyCardScript {}
    }
}

impl Script for KeyCardScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                let v_keycard_src = world.borrow::<View<PropKeySrc>>().unwrap();
                let maybe_keycard = v_keycard_src.get(entity_id);
                let acquire_key_card = {
                    if let Ok(key_card) = maybe_keycard {
                        Effect::AcquireKeyCard {
                            key_card: key_card.0.clone(),
                        }
                    } else {
                        Effect::NoEffect
                    }
                };

                let destroy_self = Effect::DestroyEntity { entity_id };
                Effect::Combined {
                    effects: vec![acquire_key_card, destroy_self],
                }
            }
            // Does turn off need to be done for email?
            _ => Effect::NoEffect,
        }
    }
}
