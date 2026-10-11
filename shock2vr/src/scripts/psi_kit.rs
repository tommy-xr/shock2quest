use shipyard::{EntityId, UniqueView, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The retail psi hypo (`PsiKitScript`): inventory use restores 30 psi on Easy, 20 otherwise, and
/// consumes one unit from its stack. `PsiFullScript` uses the same behavior with
/// a 100-point budget on every difficulty.
///
/// At an already-full pool the original reports `misc\\PsiMaxed` and leaves
/// the stack untouched. This port does not yet have a generic HUD-text effect,
/// but preserves the consequential behavior: no refill and no consumption.
pub struct PsiKitScript {
    kind: PsiKitKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PsiKitKind {
    Hypo,
    AnnelidPatch,
}

impl PsiKitScript {
    pub fn new(kind: PsiKitKind) -> Self {
        Self { kind }
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
        if !matches!(msg, MessagePayload::Frob)
            || super::gui::object_state(world, entity_id) != dark::properties::ObjectState::Normal
        {
            return Effect::NoEffect;
        }

        Effect::UsePsiKit {
            entity_id,
            amount: super::healing_item::HealingItemScript::retail_amount(
                match self.kind {
                    // Retail allobjs.osm: PsiFullScript overrides the amount
                    // getter (0x10022ad0) with 100, bypassing the hypo's Easy
                    // multiplier. Its inherited frob still applies Pharmo-Friendly.
                    PsiKitKind::AnnelidPatch => 100,
                    PsiKitKind::Hypo => {
                        if world
                            .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
                            .is_ok_and(|q| q.difficulty() == dark::gamesys::Difficulty::Easy)
                        {
                            30
                        } else {
                            20
                        }
                    }
                },
                super::healing_item::HealingItemScript::pharmo_friendly(world),
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

    use super::{PsiKitKind, PsiKitScript};

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
            let effect = PsiKitScript::new(PsiKitKind::Hypo).handle_message(
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
    fn pharmo_friendly_increases_psi_hypo_by_twenty_percent() {
        let (world, booster) = world_with_booster();
        let mut quests = crate::quest_info::QuestInfo::new();
        quests
            .player_stats_mut()
            .add_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY);
        world.add_unique(quests);
        let effect = PsiKitScript::new(PsiKitKind::Hypo).handle_message(
            booster,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(effect, Effect::UsePsiKit { amount: 24, .. }));
    }

    #[test]
    fn frob_requests_one_atomic_twenty_point_use() {
        let (world, booster) = world_with_booster();
        let effect = PsiKitScript::new(PsiKitKind::Hypo).handle_message(
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
    fn registered_annelid_patch_restores_one_hundred_with_only_the_trait_bonus() {
        for difficulty in dark::gamesys::Difficulty::ALL {
            for pharmo in [false, true] {
                let (world, patch) = world_with_booster();
                let mut quests = crate::quest_info::QuestInfo::with_difficulty(difficulty);
                if pharmo {
                    quests
                        .player_stats_mut()
                        .add_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY);
                }
                world.add_unique(quests);
                let mut script = crate::scripts::ScriptWorld::create_script("PsiFullScript".into());
                assert!(matches!(script.initialize(patch, &world), Effect::NoEffect));
                let effect = script.handle_message(
                    patch,
                    &world,
                    &PhysicsWorld::new(),
                    &MessagePayload::Frob,
                );
                assert!(matches!(effect, Effect::UsePsiKit { entity_id, amount }
                    if entity_id == patch && amount == if pharmo { 120 } else { 100 }));
            }
        }
    }

    #[test]
    fn psi_items_require_normal_state_before_consumption() {
        use dark::properties::{ObjectState, PropObjState};
        for kind in [PsiKitKind::Hypo, PsiKitKind::AnnelidPatch] {
            for state in [
                ObjectState::Unresearched,
                ObjectState::Broken,
                ObjectState::Destroyed,
                ObjectState::Locked,
                ObjectState::Hacked,
                ObjectState::Unknown(17),
            ] {
                let (mut world, item) = world_with_booster();
                world.add_component(item, PropObjState(state));
                let mut script = PsiKitScript::new(kind);
                assert!(matches!(
                    script.handle_message(
                        item,
                        &world,
                        &PhysicsWorld::new(),
                        &MessagePayload::Frob
                    ),
                    Effect::NoEffect
                ));
                world.add_component(item, PropObjState(ObjectState::Normal));
                assert!(matches!(
                    script.handle_message(
                        item,
                        &world,
                        &PhysicsWorld::new(),
                        &MessagePayload::Frob
                    ),
                    Effect::UsePsiKit { .. }
                ));
            }
        }
    }

    #[test]
    fn unrelated_messages_do_nothing() {
        let (world, booster) = world_with_booster();
        let effect = PsiKitScript::new(PsiKitKind::Hypo).handle_message(
            booster,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: booster },
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
