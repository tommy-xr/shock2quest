use shipyard::{EntityId, World};

use super::{Effect, MessagePayload, Script};
use crate::physics::PhysicsWorld;

/// Inventory use installs the cartridge; world pickup still uses its MOVE frob.
pub struct MiniGameCart;

impl Script for MiniGameCart {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => Effect::InstallMiniGames { entity_id },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepig_cart_inventory_use_installs_but_unrelated_messages_do_not() {
        let mut world = World::new();
        let entity = world.add_entity(dark::properties::PropMiniGames(0x100));
        let physics = PhysicsWorld::new();
        assert!(
            matches!(MiniGameCart.handle_message(entity, &world, &physics,
            &MessagePayload::Frob), Effect::InstallMiniGames { entity_id } if entity_id == entity)
        );
        assert!(matches!(
            MiniGameCart.handle_message(
                entity,
                &world,
                &physics,
                &MessagePayload::TurnOn { from: entity }
            ),
            Effect::NoEffect
        ));
    }
}
