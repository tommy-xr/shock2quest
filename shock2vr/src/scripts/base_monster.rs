use dark::properties::{Link, PropAI, PropAISignalResponse};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, NoopScript, Script,
    ai::{AnimatedMonsterAI, CameraAI, TurretAI},
    script_util,
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
        let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

        let maybe_ai_signal_resp = script_util::get_first_link_with_template_and_data(
            world,
            entity_id,
            |link| match link {
                Link::AIWatchObj(data) => Some(data.clone()),
                _ => None,
            },
        );

        let maybe_prop_ai = v_ai.get(entity_id);

        if maybe_prop_ai.is_err() {
            return Effect::NoEffect;
        }

        let prop_ai = maybe_prop_ai.unwrap();

        let ai: Box<dyn Script> =
            if v_prop_sig_resp.get(entity_id).is_ok() || maybe_ai_signal_resp.is_some() {
                Box::new(AnimatedMonsterAI::idle())
            } else {
                // AI scripts are created here based on PropAI value, not in the main
                // script factory in mod.rs. This is because AI entities use BaseMonster
                // as their script, which then delegates to the appropriate AI implementation.
                match prop_ai.0.to_ascii_lowercase().as_str() {
                    "camera" => Box::new(CameraAI::new()),
                    "melee" => Box::new(AnimatedMonsterAI::new()),
                    "ranged" => Box::new(AnimatedMonsterAI::new()),
                    "rangedmelee" => Box::new(AnimatedMonsterAI::new()),
                    "rangedexplode" => Box::new(AnimatedMonsterAI::new()),
                    "protocol" => Box::new(AnimatedMonsterAI::new()),
                    "shockdefault" => Box::new(AnimatedMonsterAI::new()),
                    "turret" => Box::new(TurretAI::new()),
                    //TODO:
                    "grub" => Box::new(NoopScript {}),
                    "swarmer" => Box::new(NoopScript {}),

                    _ => Box::new(AnimatedMonsterAI::idle()),
                }
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
