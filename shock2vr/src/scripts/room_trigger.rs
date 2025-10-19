use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{
    script_util::send_to_all_switch_links, trap_new_tripwire::TrapNewTripwire, Effect,
    MessagePayload, Script,
};

pub struct RoomTrigger {
    trap_tripwire: TrapNewTripwire,
}

impl RoomTrigger {
    pub fn new() -> RoomTrigger {
        RoomTrigger {
            trap_tripwire: TrapNewTripwire::new(),
        }
    }
}

impl Script for RoomTrigger {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.trap_tripwire.initialize(entity_id, world);
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let tripwire_effect = self
            .trap_tripwire
            .handle_message(entity_id, world, _physics, msg);

        let trigger_effect = {
            match msg {
                // Forward begin / end intersect messages
                MessagePayload::SensorBeginIntersect { with: _ } => {
                    send_to_all_switch_links(world, entity_id, msg.clone())
                }
                MessagePayload::SensorEndIntersect { with: _ } => {
                    send_to_all_switch_links(world, entity_id, msg.clone())
                }
                _ => Effect::NoEffect,
            }
        };

        Effect::Combined {
            effects: vec![tripwire_effect, trigger_effect],
        }
    }
}
