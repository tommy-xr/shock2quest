use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The retail psi hypo (`PsiKitScript`): inventory use restores 20 psi and
/// consumes one unit from its stack.
///
/// At an already-full pool the original reports `misc\\PsiMaxed` and leaves
/// the stack untouched. This port does not yet have a generic HUD-text effect,
/// but preserves the consequential behavior: no refill and no consumption.
pub struct PsiKitScript;

impl PsiKitScript {
    /// Retail `PsiKitScript` uses a 20-point psi bonus (also stated for the Psi
    /// Hypo in the original System Shock 2 manual).
    const RESTORE_AMOUNT: i32 = 20;

    pub fn new() -> Self {
        Self
    }
}

impl Script for PsiKitScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }

        Effect::UsePsiKit {
            entity_id,
            amount: Self::RESTORE_AMOUNT,
        }
    }
}

#[cfg(test)]
mod tests {
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::PsiKitScript;

    fn world_with_booster() -> (World, shipyard::EntityId) {
        let mut world = World::new();
        let booster = world.add_entity(());
        (world, booster)
    }

    #[test]
    fn frob_requests_one_atomic_twenty_point_use() {
        let (world, booster) = world_with_booster();
        let effect = PsiKitScript::new().handle_message(
            booster,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::UsePsiKit {
                entity_id,
                amount: 20
            } if entity_id == booster
        ));
    }

    #[test]
    fn unrelated_messages_do_nothing() {
        let (world, booster) = world_with_booster();
        let effect = PsiKitScript::new().handle_message(
            booster,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: booster },
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
