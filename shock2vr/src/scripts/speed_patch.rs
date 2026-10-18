//! `SpeedPatch` (retail `cScr_SpeedPatch`): using the Speed Boost hypo starts
//! a 20 s boost (24 with Pharmo-Friendly) and consumes one from the stack.
//! What the boost does is decided by `speed_boost`.
use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct SpeedPatch;

impl Script for SpeedPatch {
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
        Effect::UseSpeedBoost {
            entity_id,
            duration: super::healing_item::HealingItemScript::retail_duration(
                world,
                crate::speed_boost::HYPO_DURATION,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn use_patch(world: &World) -> Effect {
        let patch = world
            .borrow::<shipyard::EntitiesViewMut>()
            .unwrap()
            .add_entity((), ());
        SpeedPatch.handle_message(patch, world, &PhysicsWorld::new(), &MessagePayload::Frob)
    }

    #[test]
    fn hypo_lasts_twenty_seconds_or_twenty_four_with_pharmo_friendly() {
        let world = World::new();
        world.add_unique(crate::quest_info::QuestInfo::new());
        let Effect::UseSpeedBoost { duration, .. } = use_patch(&world) else {
            panic!("expected a speed boost");
        };
        assert_eq!(duration, Duration::from_secs(20));

        world
            .borrow::<shipyard::UniqueViewMut<crate::quest_info::QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .add_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY);
        let Effect::UseSpeedBoost { duration, .. } = use_patch(&world) else {
            panic!("expected a speed boost");
        };
        assert_eq!(duration, Duration::from_secs(24));
    }
}
