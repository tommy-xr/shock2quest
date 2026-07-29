use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, GlobalEffect, MessagePayload, Script};

/// Retail finale terminus reached after SHODAN's authored death delay and
/// SOLUS quest-bit guard.
pub struct DieShodanDie;

impl DieShodanDie {
    pub fn new() -> Self {
        Self
    }
}

impl Script for DieShodanDie {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => Effect::GlobalEffect(GlobalEffect::CompleteCampaign),
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_on_completes_the_campaign() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let source = world.add_entity(());
        let mut script = DieShodanDie::new();

        let effect = script.handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: source },
        );

        assert!(matches!(
            effect,
            Effect::GlobalEffect(GlobalEffect::CompleteCampaign)
        ));
    }

    #[test]
    fn turn_off_does_not_complete_the_campaign() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let source = world.add_entity(());
        let mut script = DieShodanDie::new();

        let effect = script.handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOff { from: source },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
