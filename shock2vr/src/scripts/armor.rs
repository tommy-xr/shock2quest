use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{physics::PhysicsWorld, time::Time};
use shipyard::{EntityId, World};

/// WormSkin retail OSM 0x1000b920/0x1000bb10 grants +2 PSI and schedules
/// PsiDrain every 30 seconds. The applier spends one point, or one HP at zero.
#[derive(Default)]
pub struct Armor {
    elapsed: f64,
}
impl Script for Armor {
    fn handle_message(
        &mut self,
        id: EntityId,
        _world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => Effect::ToggleArmor { entity_id: id },
            MessagePayload::TurnOn { .. } => Effect::EquipArmor { entity_id: id },
            MessagePayload::TurnOff { .. } => {
                self.elapsed = 0.0;
                Effect::UnequipArmor { entity_id: id }
            }
            _ => Effect::NoEffect,
        }
    }
    fn update(&mut self, id: EntityId, world: &World, _: &PhysicsWorld, time: &Time) -> Effect {
        if crate::armor::needs_unequip(world, id) {
            self.elapsed = 0.0;
            return Effect::UnequipArmor { entity_id: id };
        }
        if crate::armor::active(world) != Some(id) || !crate::armor::worm(world, id) {
            self.elapsed = 0.0;
            return Effect::NoEffect;
        }
        self.elapsed += time.elapsed.as_secs_f64();
        let ticks = (self.elapsed / 30.0).floor() as i32;
        self.elapsed -= f64::from(ticks) * 30.0;
        if ticks > 0 {
            Effect::DrainWormArmor {
                entity_id: id,
                ticks,
            }
        } else {
            Effect::NoEffect
        }
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.armor")
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.elapsed, "shock2vr.armor")
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.elapsed = state.decode(1, "shock2vr.armor")?;
        Ok(())
    }
}
