use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Retail's basic-training hypo lesson sets the player's health to 15.
/// Follow the authored TurnOn links rather than damaging the player on load.
pub struct ReduceHp;

impl Script for ReduceHp {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => Effect::SetPlayerHitPoints { hit_points: 15 },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_turn_on_assigns_the_training_health_pool() {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let id = EntityId::dead();
        for message in [MessagePayload::Frob, MessagePayload::TurnOff { from: id }] {
            assert!(matches!(
                ReduceHp.handle_message(id, &world, &physics, &message),
                Effect::NoEffect
            ));
        }
        assert!(matches!(
            ReduceHp.handle_message(id, &world, &physics, &MessagePayload::TurnOn { from: id }),
            Effect::SetPlayerHitPoints { hit_points: 15 }
        ));
    }
}
