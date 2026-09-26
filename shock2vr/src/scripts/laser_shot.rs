//! Delayed energy-bolt visibility (`LaserShot`, `Timer(RenderMe)`).
//!
//! Telliamed documents the transition; the shipped 25AE allobjs binary schedules
//! RenderMe at 50 ms, sets the root RenderType to 2 (FullBright), and incoming
//! ParticleAttachement sources to 0 (Normal). NoRender is only the initial state.
use std::time::Duration;

use dark::properties::{PropParticleGroup, RenderType};
use shipyard::{EntityId, IntoIter, IntoWithId, View, World};

use crate::{physics::PhysicsWorld, runtime_props::RuntimePropAttachment, time::Time};

use super::{Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

const STATE_KEY: &str = "shock2vr.laser_shot";

pub struct LaserShot {
    remaining: Option<Duration>,
}

impl LaserShot {
    pub fn new() -> Self {
        Self {
            remaining: Some(Duration::from_millis(50)),
        }
    }
}

impl Script for LaserShot {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let Some(remaining) = self.remaining else {
            return Effect::NoEffect;
        };
        let remaining = remaining.saturating_sub(time.elapsed);
        if !remaining.is_zero() {
            self.remaining = Some(remaining);
            return Effect::NoEffect;
        }
        self.remaining = None;
        let mut effects = vec![Effect::SetRenderType {
            entity_id,
            render_type: RenderType::FullBright,
        }];
        let (attachments, particles) = world
            .borrow::<(View<RuntimePropAttachment>, View<PropParticleGroup>)>()
            .unwrap();
        for (rider, (attachment, _)) in (&attachments, &particles).iter().with_id() {
            if attachment.parent == entity_id {
                effects.push(Effect::SetRenderType {
                    entity_id: rider,
                    render_type: RenderType::Normal,
                });
            }
        }
        Effect::combine(effects)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn reveal_waits_fifty_milliseconds_and_only_runs_once() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut script = LaserShot::new();
        let mut time = Time {
            elapsed: Duration::from_millis(49),
            ..Time::default()
        };
        assert!(matches!(
            script.update(entity, &world, &physics, &time),
            Effect::NoEffect
        ));
        time.elapsed = Duration::from_millis(1);
        let mut effects = Effect::flatten(vec![script.update(entity, &world, &physics, &time)]);
        assert_eq!(effects.len(), 1);
        let effect = effects.pop().unwrap();
        assert!(
            matches!(effect, Effect::SetRenderType { entity_id, render_type: RenderType::FullBright } if entity_id == entity)
        );
        assert!(matches!(
            script.update(entity, &world, &physics, &time),
            Effect::NoEffect
        ));
    }

    #[test]
    fn save_restore_preserves_pending_and_completed_reveal() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let physics = PhysicsWorld::new();
        let time = Time {
            elapsed: Duration::from_millis(25),
            ..Time::default()
        };
        let context_map = HashMap::new();
        let context = ScriptRestoreContext::new(&context_map);
        let mut script = LaserShot::new();
        assert!(matches!(
            script.update(entity, &world, &physics, &time),
            Effect::NoEffect
        ));
        let mut restored = LaserShot::new();
        restored
            .restore_state(&script.save_state().unwrap(), &context)
            .unwrap();
        assert!(matches!(
            Effect::flatten(vec![restored.update(entity, &world, &physics, &time)]).as_slice(),
            [Effect::SetRenderType { .. }]
        ));
        script
            .restore_state(&restored.save_state().unwrap(), &context)
            .unwrap();
        assert!(matches!(
            script.update(entity, &world, &physics, &time),
            Effect::NoEffect
        ));
    }
}
