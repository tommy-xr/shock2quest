//! A French-Epstein device opens a choice, never consumes itself on activation.
use super::{Effect, MessagePayload, Script};
use shipyard::{EntityId, World};

pub struct FreeModify;

impl Script for FreeModify {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &crate::physics::PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }
        // The mission owns the selected ammo readout. Resolve its explicit
        // gun there so dual wielding never silently prefers the other hand.
        Effect::OpenWeaponSettings {
            weapon: None,
            upgrade_device: Some(entity_id),
            from_inventory: false,
        }
    }
}
