use dark::properties::PropPosition;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{script_util::get_all_switch_links, Effect, MessagePayload, Script};

pub struct TrapTeleport {}
impl TrapTeleport {
    pub fn new() -> TrapTeleport {
        TrapTeleport {}
    }
}
impl Script for TrapTeleport {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_position = world.borrow::<View<PropPosition>>().unwrap();
                let position = v_position.get(entity_id).unwrap();

                let effs: Vec<Effect> = get_all_switch_links(world, entity_id)
                    .iter()
                    .map(|e| Effect::SetPositionRotation {
                        position: position.position,
                        rotation: position.rotation,
                        entity_id: *e,
                    })
                    .collect();

                Effect::Combined { effects: effs }
            }
            _ => Effect::NoEffect,
        }
    }
}
