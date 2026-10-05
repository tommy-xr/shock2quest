use shipyard::{EntityId, World};

use super::{Effect, MessagePayload, Script};
use crate::physics::PhysicsWorld;

/// The original trap changes its own collision properties on TurnOn only.
/// Its rendered model and interaction geometry remain present.
pub struct TrapCollideOff;

impl Script for TrapCollideOff {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => Effect::DisableObjectCollisions { entity_id },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::ScriptWorld;

    #[test]
    fn authored_script_changes_itself_on_turn_on_only() {
        let mut world = World::new();
        let target = world.add_entity(());
        let sender = world.add_entity(());
        let mut script = ScriptWorld::create_script("TrapCollideOff".into());
        let physics = PhysicsWorld::new();
        assert!(matches!(
            script.handle_message(target, &world, &physics, &MessagePayload::TurnOn { from: sender }),
            Effect::DisableObjectCollisions { entity_id } if entity_id == target
        ));
        assert!(matches!(
            script.handle_message(
                target,
                &world,
                &physics,
                &MessagePayload::TurnOff { from: sender }
            ),
            Effect::NoEffect
        ));
    }
}
