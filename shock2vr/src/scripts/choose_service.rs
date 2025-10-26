use dark::properties::PropService;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct ChooseServiceScript {}
impl ChooseServiceScript {
    pub fn new() -> ChooseServiceScript {
        ChooseServiceScript {}
    }
}

impl Script for ChooseServiceScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                // Get the service type from the P$Service property
                let v_service = world.borrow::<View<PropService>>().unwrap();
                let service = v_service.get(entity_id).map(|s| s.0).unwrap_or(0);

                // Determine which entity to trigger based on service type
                let entity_to_trigger = match service {
                    0 => Some("START_01".to_string()), // Marines
                    1 => Some("START_11".to_string()), // Navy
                    2 => Some("START_21".to_string()), // OSA
                    _ => {
                        // Unknown service type, default to Marines
                        Some("START_01".to_string())
                    }
                };

                Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    level_file: "station.mis".to_owned(),
                    loc: None,
                    entity_to_trigger,
                })
            }
            _ => Effect::NoEffect,
        }
    }
}
