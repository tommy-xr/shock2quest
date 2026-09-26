//! The object script owns the retail twenty-second lifetime; SwarmerAI owns flight.
use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
};
use crate::{physics::PhysicsWorld, time::Time};
use shipyard::{EntityId, World};
const KEY: &str = "shock2vr.swarm_lifetime";
pub struct Swarm {
    remaining: f32,
    expired: bool,
}
impl Swarm {
    pub fn new() -> Self {
        Self {
            remaining: 20.0,
            expired: false,
        }
    }
}
impl Script for Swarm {
    fn script_state_key(&self) -> Option<&'static str> {
        Some(KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &(self.remaining, self.expired), KEY)
    }
    fn restore_state(
        &mut self,
        saved: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        (self.remaining, self.expired) = saved.decode(1, KEY)?;
        Ok(())
    }
    fn update(&mut self, entity: EntityId, _: &World, _: &PhysicsWorld, time: &Time) -> Effect {
        if self.expired {
            return Effect::NoEffect;
        }
        self.remaining = (self.remaining - time.elapsed.as_secs_f32()).max(0.0);
        if self.remaining > 0.0 {
            return Effect::NoEffect;
        }
        self.expired = true;
        Effect::Send {
            msg: Message {
                to: entity,
                payload: MessagePayload::Slay,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifetime_restores_remaining_time_and_slays_once() {
        let mut swarm = Swarm::new();
        let world = World::new();
        let physics = PhysicsWorld::new();
        let entity = EntityId::from_inner(1).unwrap();
        let advance = |swarm: &mut Swarm, seconds| {
            swarm.update(
                entity,
                &world,
                &physics,
                &Time {
                    elapsed: std::time::Duration::from_secs_f32(seconds),
                    ..Time::default()
                },
            )
        };
        assert!(matches!(advance(&mut swarm, 12.0), Effect::NoEffect));
        let saved = swarm.save_state().unwrap();
        let mut restored = Swarm::new();
        restored
            .restore_state(
                &saved,
                &ScriptRestoreContext::new(&std::collections::HashMap::new()),
            )
            .unwrap();
        assert!(matches!(advance(&mut restored, 7.0), Effect::NoEffect));
        assert!(matches!(
            advance(&mut restored, 1.0),
            Effect::Send {
                msg: Message {
                    payload: MessagePayload::Slay,
                    ..
                }
            }
        ));
        assert!(matches!(advance(&mut restored, 10.0), Effect::NoEffect));
    }
}
