use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct SlotMachine;

impl Script for SlotMachine {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => Effect::PlaySlotMachine { entity_id },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::ScriptWorld;

    #[test]
    fn registered_slot_machine_gambles_only_on_use() {
        let mut world = World::new();
        let machine = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = ScriptWorld::create_script("SlotMachine".into());
        assert!(matches!(
            script.handle_message(machine, &world, &physics, &MessagePayload::Frob),
            Effect::PlaySlotMachine { entity_id } if entity_id == machine
        ));
        for msg in [
            MessagePayload::TurnOn { from: machine },
            MessagePayload::TurnOff { from: machine },
        ] {
            assert!(matches!(
                script.handle_message(machine, &world, &physics, &msg),
                Effect::NoEffect
            ));
        }
    }
}
