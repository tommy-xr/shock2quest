use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct OnceRouter {}
impl OnceRouter {
    pub fn new() -> OnceRouter {
        OnceRouter {}
    }
}
impl Script for OnceRouter {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let send_effect = send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                );
                let destroy_effect = Effect::DestroyEntity { entity_id };
                Effect::Combined {
                    effects: vec![send_effect, destroy_effect],
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
