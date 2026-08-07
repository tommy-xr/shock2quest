use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct ChemicalScript;

impl ChemicalScript {
    pub fn new() -> Self {
        Self
    }
}

impl Script for ChemicalScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => Effect::UseResearchChemical { entity_id },
            _ => Effect::NoEffect,
        }
    }
}
