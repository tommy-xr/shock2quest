use dark::properties::{CollisionType, PropCollisionType};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

// Script to handle collision type
pub struct InternalCollisionType {
    collision_flags: CollisionType,
}

impl InternalCollisionType {
    pub fn new() -> InternalCollisionType {
        InternalCollisionType {
            collision_flags: CollisionType::empty(),
        }
    }
}

impl Script for InternalCollisionType {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_collision_flags = world.borrow::<View<PropCollisionType>>().unwrap();

        if let Ok(collision_flags) = v_collision_flags.get(entity_id) {
            self.collision_flags = collision_flags.collision_type;
        }

        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with } => {
                let initial_effect = {
                    if self.collision_flags.contains(CollisionType::SLAY_ON_IMPACT) {
                        Effect::SlayEntity { entity_id }
                    } else if self
                        .collision_flags
                        .contains(CollisionType::DESTROY_ON_IMPACT)
                    {
                        Effect::DestroyEntity { entity_id }
                    } else {
                        Effect::NoEffect
                    }
                };
                let damage_effect = Effect::Send {
                    msg: Message {
                        to: *with,
                        payload: MessagePayload::Damage { amount: 1.0 },
                    },
                };
                Effect::Multiple(vec![initial_effect, damage_effect])
            }
            _ => Effect::NoEffect,
        }
    }
}
