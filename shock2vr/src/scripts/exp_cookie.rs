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
pub struct ExpCookie {
    // The module is destroyed the same frame it is first collected (effects
    // apply after the whole message batch), so a second Frob delivered in that
    // same batch - both VR hands squeezing the same loot-panel icon, or trigger
    // and squeeze from different hands - would otherwise still see it alive and
    // award its modules twice while only one DestroyEntity is ever queued.
    // Latch in-memory on the first award, as `InternalNanitesScript` and
    // `KeyCardScript` do; the entity is gone by the next frame, so this never
    // needs to survive a save.
    collected: bool,
}

impl ExpCookie {
    pub fn new() -> ExpCookie {
        ExpCookie { collected: false }
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
            MessagePayload::Frob if !self.collected => {
                self.collected = true;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Negative-first regression: the destroy effect only applies after the
    /// whole message batch, so a second Frob in the same batch - both VR hands
    /// squeezing the same loot-panel module, now that a panel squeeze collects
    /// - still finds the entity alive. Without the `collected` latch this would
    /// award the pile's modules twice.
    #[test]
    fn a_second_frob_in_the_same_batch_awards_nothing_more() {
        let mut world = World::new();
        let entity = world.add_entity(PropStackCount(10));
        let mut script = ExpCookie::new();

        let first =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);
        let second =
            script.handle_message(entity, &world, &PhysicsWorld::new(), &MessagePayload::Frob);

        let Effect::Combined { effects } = first else {
            panic!("expected a combined effect, got {first:?}");
        };
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::AwardXP { amount } if *amount == 10)),
            "the first Frob should award the pile's modules"
        );
        assert!(
            matches!(second, Effect::NoEffect),
            "a second Frob in the same batch must award nothing more, got {second:?}"
        );
    }
}
