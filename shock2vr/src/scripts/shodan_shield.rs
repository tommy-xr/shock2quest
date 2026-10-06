//! Retail ShodanShield and TransluceByDamage are independent RootScript children.
//! The arena authors both: one regenerates, the other reacts to incoming damage.
use std::time::Duration;

use dark::properties::PropMaxHitPoints;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

const REGEN_INTERVAL: Duration = Duration::from_secs(1);
const STATE_KEY: &str = "shock2vr.shodan_shield";

pub struct ShodanShield {
    remaining: Duration,
}

impl ShodanShield {
    pub fn new() -> Self {
        Self {
            remaining: REGEN_INTERVAL,
        }
    }
}

impl Script for ShodanShield {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.remaining = self.remaining.saturating_sub(time.elapsed);
        if !self.remaining.is_zero() {
            return Effect::NoEffect;
        }
        // Retail schedules a new Regen 1000ms after each callback, rather than
        // catching up several missed ticks after a long frame.
        self.remaining = REGEN_INTERVAL;
        let maximums = world.borrow::<View<PropMaxHitPoints>>().unwrap();
        let Ok(maximum) = maximums.get(entity_id) else {
            return Effect::NoEffect;
        };
        // Retail allobjs ShodanShield::OnTimer (0x1800249d0): truncate MAX_HP
        // * 0.8, then min(HP + 1, cap). Authored 115/140 therefore becomes 112.
        let cap = (maximum.hit_points as f32 * 0.8) as i32;
        Effect::combine(vec![
            Effect::RegenerateHitPoints {
                entity_id,
                amount: 1,
                cap,
            },
            Effect::SetRenderAlphaFromHitPoints { entity_id },
        ])
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.remaining, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.remaining = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[derive(Default)]
pub struct TransluceByDamage {
    damaged: bool,
}

impl Script for TransluceByDamage {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        self.damaged |= matches!(msg, MessagePayload::Damage { .. });
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if !std::mem::take(&mut self.damaged) {
            return Effect::NoEffect;
        }
        // ScriptWorld appends update effects after every message's effects.
        // Read actual HP at application time, after InternalSimpleHealth/AI
        // have applied damage, including multiple hits during this frame.
        Effect::SetRenderAlphaFromHitPoints { entity_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn step(script: &mut dyn Script, world: &World, id: EntityId, millis: u64) -> Effect {
        script.update(
            id,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_millis(millis),
                total: Duration::ZERO,
            },
        )
    }

    #[test]
    fn shield_ticks_at_one_second_and_uses_retail_truncated_cap() {
        let mut world = World::new();
        let id = world.add_entity((PropMaxHitPoints { hit_points: 141 },));
        let mut script = ShodanShield::new();
        assert!(matches!(
            step(&mut script, &world, id, 999),
            Effect::NoEffect
        ));
        let effects = Effect::flatten(vec![step(&mut script, &world, id, 1)]);
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::RegenerateHitPoints {
                    amount: 1,
                    cap: 112,
                    ..
                },
                Effect::SetRenderAlphaFromHitPoints { .. }
            ]
        ));
        assert!(matches!(
            step(&mut script, &world, id, 999),
            Effect::NoEffect
        ));
        assert_eq!(
            Effect::flatten(vec![step(&mut script, &world, id, 1)]).len(),
            2
        );
    }

    #[test]
    fn saved_timer_retains_subsecond_remainder() {
        let mut world = World::new();
        let id = world.add_entity((PropMaxHitPoints { hit_points: 140 },));
        let mut script = ShodanShield::new();
        step(&mut script, &world, id, 400);
        let serialized = serde_json::to_string(&script.save_state().unwrap()).unwrap();
        let mut restored = ShodanShield::new();
        restored
            .restore_state(
                &serde_json::from_str(&serialized).unwrap(),
                &ScriptRestoreContext::new(&HashMap::new()),
            )
            .unwrap();
        restored.initialize_after_hydration(id, &world, true);
        assert!(matches!(
            step(&mut restored, &world, id, 599),
            Effect::NoEffect
        ));
        assert_eq!(
            Effect::flatten(vec![step(&mut restored, &world, id, 1)]).len(),
            2
        );
    }

    #[test]
    fn damage_requests_one_post_damage_alpha_sync_and_ignores_other_messages() {
        let mut world = World::new();
        let id = world.add_entity(());
        let mut script = TransluceByDamage::default();
        script.handle_message(id, &world, &PhysicsWorld::new(), &MessagePayload::Frob);
        assert!(matches!(step(&mut script, &world, id, 1), Effect::NoEffect));
        for _ in 0..2 {
            assert!(matches!(
                script.handle_message(
                    id,
                    &world,
                    &PhysicsWorld::new(),
                    &MessagePayload::Damage {
                        amount: 10.0,
                        impact: None
                    }
                ),
                Effect::NoEffect
            ));
        }
        assert!(
            matches!(step(&mut script, &world, id, 1), Effect::SetRenderAlphaFromHitPoints { entity_id } if entity_id == id)
        );
        assert!(matches!(step(&mut script, &world, id, 1), Effect::NoEffect));
    }
}
