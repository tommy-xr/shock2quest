use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};
use crate::scripts::gui::{TRAIT_PHARMO_FRIENDLY, pharmo_friendly_amount, player_has_os_trait};

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
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }

        Effect::UsePsiKit {
            entity_id,
            amount: pharmo_friendly_amount(
                Self::RESTORE_AMOUNT,
                player_has_os_trait(world, TRAIT_PHARMO_FRIENDLY),
            ),
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
    fn pharmo_friendly_boosts_the_hypo_by_a_fifth() {
        use crate::quest_info::QuestInfo;
        use crate::scripts::gui::TRAIT_PHARMO_FRIENDLY;

        let (world, booster) = world_with_booster();
        let mut quests = QuestInfo::new();
        quests
            .player_stats_mut()
            .add_os_trait(TRAIT_PHARMO_FRIENDLY);
        world.add_unique(quests);

        let effect = PsiKitScript::new().handle_message(
            booster,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(
            matches!(effect, Effect::UsePsiKit { amount: 24, .. }),
            "Pharmo-Friendly turns the 20-point hypo into 24"
        );
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
