use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    ai::{AnimatedMonsterAI, CameraAI},
    Effect, MessagePayload, Script,
};

pub struct BaseMonster {
    ai: Box<dyn Script>,
}
impl BaseMonster {
    pub fn new() -> BaseMonster {
        BaseMonster {
            ai: Box::new(AnimatedMonsterAI::new()),
        }
    }
}
impl Script for BaseMonster {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.ai.initialize(entity_id, world)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.ai.update(entity_id, world, physics, time)
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        self.ai.handle_message(entity_id, world, physics, msg)
    }
}
