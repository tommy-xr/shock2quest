use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{implants, physics::PhysicsWorld, runtime_props::RuntimePropImplantSlot, time::Time};
use dark::properties::{PropDrainAmount, PropDrainRate};
use shipyard::{EntityId, Get, View, World};

const STATE_KEY: &str = "shock2vr.implant";
#[derive(Default)]
pub struct Implant {
    elapsed: f32,
}
impl Script for Implant {
    fn handle_message(
        &mut self,
        id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        message: &MessagePayload,
    ) -> Effect {
        match message {
            MessagePayload::Frob => Effect::ToggleImplant { entity_id: id },
            MessagePayload::Drop => {
                self.elapsed = 0.0;
                Effect::UnequipImplant { entity_id: id }
            }
            MessagePayload::Recharge => Effect::AdjustImplantEnergy {
                entity_id: id,
                amount: implants::recharge_capacity(world),
                recharge: true,
            },
            _ => Effect::NoEffect,
        }
    }
    fn update(
        &mut self,
        id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if !world
            .borrow::<View<RuntimePropImplantSlot>>()
            .is_ok_and(|v| v.contains(id))
        {
            self.elapsed = 0.0;
            return Effect::NoEffect;
        }
        if !implants::equipped(world).contains(&Some(id)) {
            self.elapsed = 0.0;
            return Effect::UnequipImplant { entity_id: id };
        }
        if implants::energy(world, id) <= 0.0 {
            return Effect::NoEffect;
        }
        let rate = world
            .borrow::<View<PropDrainRate>>()
            .ok()
            .and_then(|v| v.get(id).ok().map(|p| p.0))
            .unwrap_or(10.0);
        let amount = world
            .borrow::<View<PropDrainAmount>>()
            .ok()
            .and_then(|v| v.get(id).ok().map(|p| p.0))
            .unwrap_or(1.0);
        if !rate.is_finite() || rate <= 0.0 || !amount.is_finite() || amount <= 0.0 {
            return Effect::NoEffect;
        }
        self.elapsed += time.elapsed.as_secs_f32();
        let ticks = (self.elapsed / rate).floor();
        self.elapsed -= ticks * rate;
        if ticks > 0.0 {
            Effect::AdjustImplantEnergy {
                entity_id: id,
                amount: -ticks * amount,
                recharge: false,
            }
        } else {
            Effect::NoEffect
        }
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.elapsed, STATE_KEY)
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.elapsed = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{Link, Links, PropEnergy, PropImplantDesc, ToLink, WrappedEntityId};
    use std::{collections::HashMap, time::Duration};

    #[test]
    fn drain_preserves_timer_phase_across_save_and_stops_when_removed() {
        let mut world = World::new();
        let implant = world.add_entity((
            PropImplantDesc(0),
            PropEnergy(100.0),
            PropDrainRate(10.0),
            PropDrainAmount(1.0),
            RuntimePropImplantSlot(0),
        ));
        let player = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(implant)),
                link: Link::Contains(0),
            }],
        });
        world.add_unique(crate::mission::PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let physics = PhysicsWorld::new();
        let step = |script: &mut Implant, world: &World, seconds| {
            script.update(
                implant,
                world,
                &physics,
                &Time {
                    elapsed: Duration::from_secs(seconds),
                    total: Duration::from_secs(seconds),
                },
            )
        };
        let mut original = Implant::default();
        assert!(matches!(step(&mut original, &world, 9), Effect::NoEffect));
        let mut restored = Implant::default();
        restored
            .restore_state(
                &original.save_state().unwrap(),
                &ScriptRestoreContext::new(&HashMap::new()),
            )
            .unwrap();
        assert!(matches!(
            step(&mut restored, &world, 1),
            Effect::AdjustImplantEnergy {
                amount: -1.0,
                recharge: false,
                ..
            }
        ));
        assert!(matches!(
            step(&mut restored, &world, 25),
            Effect::AdjustImplantEnergy { amount: -2.0, .. }
        ));
        world.add_component(implant, PropEnergy(0.0));
        assert!(matches!(step(&mut restored, &world, 10), Effect::NoEffect));
        world.remove::<(RuntimePropImplantSlot,)>(implant);
        assert!(matches!(step(&mut restored, &world, 10), Effect::NoEffect));
        assert_eq!(restored.elapsed, 0.0);
    }
}
