use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{script_util::send_to_all_switch_links, Effect, MessagePayload, Script};

pub struct OnceRoom {}
impl OnceRoom {
    pub fn new() -> OnceRoom {
        OnceRoom {}
    }
}

impl Script for OnceRoom {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::SensorBeginIntersect { with: _ } => {
                let switch_eff = send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                );
                let destroy_eff = Effect::DestroyEntity { entity_id };

                Effect::Combined {
                    effects: vec![switch_eff, destroy_eff],
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
