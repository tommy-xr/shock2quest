use dark::properties::PropSignalType;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, scripts::script_util};

use super::{Effect, MessagePayload, Script};

pub struct TrapSignal;
impl TrapSignal {
    pub fn new() -> TrapSignal {
        TrapSignal
    }
}
impl Script for TrapSignal {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_prop_signal_type = world.borrow::<View<PropSignalType>>().unwrap();
                if let Ok(prop_signal_type) = v_prop_signal_type.get(entity_id) {
                    script_util::send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::Signal {
                            name: prop_signal_type.0.clone(),
                        },
                    )
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
