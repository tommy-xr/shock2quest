use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script,
    script_util::{change_to_first_model, change_to_last_model},
};

pub struct TweqDepressable {
    time_to_revert: Option<f32>,
}
impl TweqDepressable {
    pub fn new() -> TweqDepressable {
        TweqDepressable {
            time_to_revert: None,
        }
    }
}
impl Script for TweqDepressable {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                self.time_to_revert = Some(3.0); // TODO: How much time to switch back?
                change_to_last_model(world, entity_id)
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if let Some(remaining_time) = self.time_to_revert {
            let new_remaining_time = remaining_time - time.elapsed.as_secs_f32();
            if new_remaining_time < 0.0 {
                self.time_to_revert = None;
                change_to_first_model(world, entity_id)
            } else {
                self.time_to_revert = Some(new_remaining_time);
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        }
    }
}
