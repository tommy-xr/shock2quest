use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

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
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with } => Effect::Send {
                msg: Message {
                    to: *with,
                    payload: MessagePayload::Damage { amount: 1.0 },
                },
            },
            _ => Effect::NoEffect,
        }
    }
}
