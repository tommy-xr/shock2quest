use dark::properties::{PropPosition, TeleportSource};
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
        msg: &MessagePayload,
    ) -> Effect {
        // Teleport on TurnOn only, like the original engine. Firing on any
        // message meant an unrelated TurnOff (e.g. a tripwire's EXIT edge)
        // teleported the player a second time.
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }

        let v_position = world.borrow::<View<PropPosition>>().unwrap();

        let maybe_position = v_position.get(entity_id);

        if let Ok(position) = maybe_position {
            Effect::SetPlayerPosition {
                position: position.position,
                is_teleport: true,
                // Scripted trap arrival: don't re-fire the tripwire we land in
                // (e.g. the earth.mis montage return teleport, #515).
                source: TeleportSource::ScriptedTrap,
            }
        } else {
            Effect::NoEffect
        }
    }
}
