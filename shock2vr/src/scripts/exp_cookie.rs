use dark::properties::{PropExp, PropStackCount};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Cyber-module pickup ("fake cookie" in the retail source - the pickup script
/// is literally named `expcookie`). Frobbing the module awards its worth of
/// cyber modules to the player and removes the object from the world, mirroring
/// [`super::trap_exp_once::TrapEXPOnce`] but driven by a player frob rather than
/// a trap `TurnOn`.
///
/// The award amount is the object's stack count (`P$StackCoun` - the retail
/// engine stores an EXP-cookie pile's module value as its stack count, e.g. the
/// "10 EXP" pile has stack count 10), falling back to `P$ExP` for any
/// cookie authored the trap way.
pub struct ExpCookie {}

impl ExpCookie {
    pub fn new() -> ExpCookie {
        ExpCookie {}
    }
}

impl Script for ExpCookie {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                let stack = world
                    .borrow::<View<PropStackCount>>()
                    .unwrap()
                    .get(entity_id)
                    .map(|p| p.0)
                    .ok();
                let award_amount = stack
                    .filter(|&n| n > 0)
                    .or_else(|| {
                        world
                            .borrow::<View<PropExp>>()
                            .unwrap()
                            .get(entity_id)
                            .map(|p| p.0)
                            .ok()
                    })
                    .unwrap_or(0);
                Effect::Combined {
                    effects: vec![
                        Effect::AwardXP {
                            amount: award_amount,
                        },
                        Effect::DestroyEntity { entity_id },
                    ],
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
