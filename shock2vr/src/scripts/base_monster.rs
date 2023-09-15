use dark::properties::PropAI;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    ai::{AnimatedMonsterAI, CameraAI},
    Effect, MessagePayload, NoopScript, Script,
};

pub struct BaseMonster {
    ai: Box<dyn Script>,
}
impl BaseMonster {
    pub fn new() -> BaseMonster {
        BaseMonster {
            ai: Box::new(NoopScript {}),
        }
    }
}
impl Script for BaseMonster {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_ai = world.borrow::<View<PropAI>>().unwrap();

        let maybe_prop_ai = v_ai.get(entity_id);

        if maybe_prop_ai.is_err() {
            return Effect::NoEffect;
        }

        let prop_ai = maybe_prop_ai.unwrap();

        let ai: Box<dyn Script> = match prop_ai.0.to_ascii_lowercase().as_str() {
            "camera" => Box::new(CameraAI::new()),
            "melee" => Box::new(AnimatedMonsterAI::new()),
            "ranged" => Box::new(AnimatedMonsterAI::new()),
            "rangedmelee" => Box::new(AnimatedMonsterAI::new()),
            "rangedexplode" => Box::new(AnimatedMonsterAI::new()),
            "protocol" => Box::new(AnimatedMonsterAI::new()),
            "shockdefault" => Box::new(AnimatedMonsterAI::new()),
            //TODO:
            "grub" => Box::new(NoopScript {}),
            "swarmer" => Box::new(NoopScript {}),
            "turret" => Box::new(NoopScript {}),

            _ => Box::new(NoopScript {}),
        };

        self.ai = ai;

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
