use serde::{Deserialize, Serialize};
use shipyard::{EntityId, UniqueView, World};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{mission::mission_core::PlayerInfo, physics::PhysicsWorld, time::Time};

const KEY: &str = "shock2vr.summoned_psi_sword";

#[derive(Serialize, Deserialize)]
struct Summon {
    amp: Option<u64>,
    remaining_secs: f32,
}

/// The summoned blade owns its lifetime, including across saves and transitions.
/// Ordinary PsiSword objects in debug scenes have no summon and never expire.
#[derive(Default)]
pub struct SummonedPsiSword {
    summon: Option<Summon>,
}

impl Script for SummonedPsiSword {
    fn handle_message(
        &mut self,
        _id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if let MessagePayload::BeginPsiSword { amp, duration_secs } = msg {
            self.summon = Some(Summon {
                amp: Some(amp.inner()),
                remaining_secs: *duration_secs,
            });
        }
        Effect::NoEffect
    }

    fn update(
        &mut self,
        id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let Some(summon) = self.summon.as_mut() else {
            return Effect::NoEffect;
        };
        summon.remaining_secs -= time.elapsed.as_secs_f32();
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let held =
            player.left_hand_entity_id == Some(id) || player.right_hand_entity_id == Some(id);
        // Putting the blade away cancels it without replacing the new weapon.
        if summon.remaining_secs <= 0.0 || !held {
            let amp = summon.amp.and_then(EntityId::from_inner);
            self.summon = None;
            return Effect::FinishPsiSword { blade: id, amp };
        }
        if !world
            .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
            .unwrap()
            .is_active(-1119)
        {
            // The generic active-power list is runtime-only; reconstruct this
            // blade's entry from its saved timer after hydration.
            return Effect::ActivatePsiPower {
                template_id: -1119,
                name: "Psi Sword".to_owned(),
                duration_secs: summon.remaining_secs,
            };
        }
        Effect::NoEffect
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.summon, KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.summon = state.decode(1, KEY)?;
        if let Some(summon) = self.summon.as_mut() {
            summon.amp = match summon.amp.map(|id| context.remap_entity(id)).transpose() {
                Ok(amp) => amp.map(EntityId::inner),
                // A full backpack can leave the displaced amp in the previous
                // level. Losing that reference must not prevent loading the
                // carried blade, nor resurrect the abandoned amplifier.
                Err(ScriptStateError::MissingEntityReference(_)) => None,
                Err(error) => return Err(error),
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Quaternion, vec3};
    use std::{collections::HashMap, time::Duration};

    #[test]
    fn save_restores_timer_and_remaps_amp_before_expiry() {
        let mut world = World::new();
        let old_amp = world.add_entity(());
        let amp = world.add_entity(());
        let blade = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: old_amp,
            left_hand_entity_id: Some(blade),
            right_hand_entity_id: None,
            inventory_entity_id: old_amp,
        });
        world.add_unique(crate::psi::ActivePsiPowers::default());
        let before = SummonedPsiSword {
            summon: Some(Summon {
                amp: Some(old_amp.inner()),
                remaining_secs: 0.5,
            }),
        };
        let mut after = SummonedPsiSword::default();
        let map = HashMap::from([(old_amp, amp)]);
        after
            .restore_state(
                &before.save_state().unwrap(),
                &ScriptRestoreContext::new(&map),
            )
            .unwrap();
        let physics = PhysicsWorld::new();
        let time = Time {
            elapsed: Duration::from_millis(250),
            total: Duration::from_millis(250),
        };
        assert!(
            matches!(after.update(blade, &world, &physics, &time), Effect::ActivatePsiPower { template_id: -1119, duration_secs, .. } if duration_secs == 0.25)
        );
        assert!(
            matches!(after.update(blade, &world, &physics, &time), Effect::FinishPsiSword { blade: b, amp: a } if b == blade && a == Some(amp))
        );
        assert!(matches!(
            after.update(blade, &world, &physics, &time),
            Effect::NoEffect
        ));
    }

    #[test]
    fn an_amp_left_in_another_level_does_not_block_blade_hydration() {
        let mut world = World::new();
        let amp = world.add_entity(());
        let before = SummonedPsiSword {
            summon: Some(Summon {
                amp: Some(amp.inner()),
                remaining_secs: 12.0,
            }),
        };
        let mut after = SummonedPsiSword::default();
        after
            .restore_state(
                &before.save_state().unwrap(),
                &ScriptRestoreContext::new(&HashMap::new()),
            )
            .unwrap();
        let restored = after.summon.unwrap();
        assert_eq!(
            restored.amp, None,
            "an absent amplifier must not be restored or duplicated"
        );
        assert_eq!(
            restored.remaining_secs, 12.0,
            "the carried blade keeps its remaining lifetime"
        );
    }
}
