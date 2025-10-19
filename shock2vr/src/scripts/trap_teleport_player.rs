use dark::properties::PropPosition;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct TrapTeleportPlayer {}
impl TrapTeleportPlayer {
    pub fn new() -> TrapTeleportPlayer {
        TrapTeleportPlayer {}
    }
}
impl Script for TrapTeleportPlayer {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        let v_position = world.borrow::<View<PropPosition>>().unwrap();

        let maybe_position = v_position.get(entity_id);

        if let Ok(position) = maybe_position {
            Effect::SetPlayerPosition {
                position: position.position,
                is_teleport: true,
            }
        } else {
            Effect::NoEffect
        }
    }
}
