use shipyard::{EntityId, UniqueView, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The retail psi hypo (`PsiKitScript`): inventory use restores 30 psi on Easy, 20 otherwise, and
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
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }

        Effect::UsePsiKit {
            entity_id,
            amount: if world
                .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
                .is_ok_and(|q| q.difficulty() == dark::gamesys::Difficulty::Easy)
            {
                30
            } else {
                Self::RESTORE_AMOUNT
            },
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
    fn easy_psi_hypo_requests_thirty_points_only_on_easy() {
        for difficulty in dark::gamesys::Difficulty::ALL {
            let (world, booster) = world_with_booster();
            world.add_unique(crate::quest_info::QuestInfo::with_difficulty(difficulty));
            let effect = PsiKitScript::new().handle_message(
                booster,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::Frob,
            );
            match effect {
                Effect::UsePsiKit { amount, .. } => assert_eq!(
                    amount,
                    if difficulty == dark::gamesys::Difficulty::Easy {
                        30
                    } else {
                        20
                    }
                ),
                _ => panic!("expected psi kit use"),
            }
        }
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
