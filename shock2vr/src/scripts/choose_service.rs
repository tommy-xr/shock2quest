use shipyard::{EntityId, World};

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
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    level_file: "medsci1.mis".to_owned(),
                    loc: None,
                })
            }
            _ => Effect::NoEffect,
        }
    }
}
