use dark::properties::PropExp;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct TrapEXPOnce {}
impl TrapEXPOnce {
    pub fn new() -> TrapEXPOnce {
        TrapEXPOnce {}
    }
}
impl Script for TrapEXPOnce {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_amount = world.borrow::<View<PropExp>>().unwrap();
                let award_amount = v_amount.get(entity_id).map(|p| p.0).unwrap_or(0);
                let award_effect = Effect::AwardXP {
                    amount: award_amount,
                };
                let destroy_effect = Effect::DestroyEntity { entity_id };
                Effect::Combined {
                    effects: vec![award_effect, destroy_effect],
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
