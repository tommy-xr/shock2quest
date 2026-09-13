use dark::properties::{Link, PropAI, PropAISignalResponse};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, NoopScript, Script,
    ai::{AnimatedMonsterAI, CameraAI, GrubAI, TurretAI},
    script_util,
};

pub struct BaseMonster {
    ai: Box<dyn Script>,
    stasis: Option<super::stasis::StasisState>,
    hydrated_ai: bool,
}

impl BaseMonster {
    pub fn new() -> BaseMonster {
        BaseMonster {
            ai: Box::new(NoopScript {}),
            stasis: None,
            hydrated_ai: false,
        }
    }
}
impl Script for BaseMonster {
    fn stasis(&self) -> Option<&super::stasis::StasisState> {
        self.stasis.as_ref()
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.ai_stasis")
    }
    fn save_state(&self) -> Result<super::ScriptState, super::ScriptStateError> {
        let child = self
            .ai
            .script_state_key()
            .map(|key| self.ai.save_state().map(|state| (key.to_owned(), state)))
            .transpose()?;
        super::ScriptState::encode(3, &(&self.stasis, child), "shock2vr.ai_stasis")
    }
    fn restore_state(
        &mut self,
        state: &super::ScriptState,
        context: &super::ScriptRestoreContext<'_>,
    ) -> Result<(), super::ScriptStateError> {
        let (stasis, child): (
            Option<super::stasis::StasisState>,
            Option<(String, super::ScriptState)>,
        ) = state.decode(3, "shock2vr.ai_stasis")?;
        self.stasis = stasis;
        self.hydrated_ai = false;
        if let Some((key, saved)) = child {
            self.ai = match key.as_str() {
                "shock2vr.grub_ai" => Box::new(GrubAI::new()),
                "shock2vr.turret" => Box::new(TurretAI::new()),
                _ => {
                    return Err(super::ScriptStateError::InvalidPayload {
                        script_key: "shock2vr.ai_stasis".into(),
                        message: format!("unknown AI state {key}"),
                    });
                }
            };
            self.ai.restore_state(&saved, context)?;
            self.hydrated_ai = true;
        }
        // Reject malformed state through the same typed decoder error path.
        if self.stasis.as_ref().is_some_and(|state| !state.valid()) {
            return Err(super::ScriptStateError::InvalidPayload {
                script_key: "shock2vr.ai_stasis".to_owned(),
                message: "invalid stasis state".to_owned(),
            });
        }
        Ok(())
    }
    fn initialize_after_hydration(
        &mut self,
        entity: EntityId,
        world: &World,
        _hydrated: bool,
    ) -> Effect {
        if self.hydrated_ai {
            self.ai.initialize_after_hydration(entity, world, true)
        } else {
            self.initialize(entity, world)
        }
    }

    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_ai = world.borrow::<View<PropAI>>().unwrap();
        let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

        let maybe_ai_signal_resp = script_util::get_first_link_with_template_and_data(
            world,
            entity_id,
            |link| match link {
                Link::AIWatchObj(data) => Some(data.clone()),
                _ => None,
            },
        );

        let maybe_prop_ai = v_ai.get(entity_id);

        if maybe_prop_ai.is_err() {
            return Effect::NoEffect;
        }

        let prop_ai = maybe_prop_ai.unwrap();

        let ai: Box<dyn Script> =
            if v_prop_sig_resp.get(entity_id).is_ok() || maybe_ai_signal_resp.is_some() {
                Box::new(AnimatedMonsterAI::idle())
            } else {
                // AI scripts are created here based on PropAI value, not in the main
                // script factory in mod.rs. This is because AI entities use BaseMonster
                // as their script, which then delegates to the appropriate AI implementation.
                match prop_ai.0.to_ascii_lowercase().as_str() {
                    "camera" => Box::new(CameraAI::new()),
                    "melee" => Box::new(AnimatedMonsterAI::new()),
                    "ranged" => Box::new(AnimatedMonsterAI::new()),
                    "rangedmelee" => Box::new(AnimatedMonsterAI::new()),
                    "rangedexplode" => Box::new(AnimatedMonsterAI::new()),
                    "protocol" => Box::new(AnimatedMonsterAI::new()),
                    "shockdefault" => Box::new(AnimatedMonsterAI::new()),
                    "turret" => Box::new(TurretAI::new()),
                    "grub" => Box::new(GrubAI::new()),
                    // TODO: flying object-model controller.
                    "swarmer" => Box::new(NoopScript {}),

                    _ => Box::new(AnimatedMonsterAI::idle()),
                }
            };

        if !self.hydrated_ai {
            self.ai = ai;
        }

        self.ai.initialize(entity_id, world)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if super::ai::ai_util::is_killed(entity_id, world) {
            self.stasis = None;
        } else if let Some(stasis) = self.stasis.as_mut() {
            if stasis.tick(time.elapsed.as_secs_f32()) {
                return Effect::NoEffect;
            }
            self.stasis = None;
        }
        self.ai.update(entity_id, world, physics, time)
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if let MessagePayload::Freeze { duration_seconds } = msg {
            if world.borrow::<View<PropAI>>().unwrap().contains(entity_id)
                && !super::ai::ai_util::is_killed(entity_id, world)
                && duration_seconds.is_finite()
            {
                if let Some(stasis) = self.stasis.as_mut() {
                    stasis.remaining_seconds = *duration_seconds;
                } else if *duration_seconds != 0.0 {
                    self.stasis = Some(super::stasis::StasisState::capture(
                        world,
                        entity_id,
                        *duration_seconds,
                    ));
                }
            }
            return Effect::NoEffect;
        }
        if matches!(msg, MessagePayload::Slay) || super::ai::ai_util::is_killed(entity_id, world) {
            self.stasis = None;
        }
        // Preserve animation bookkeeping in message order even while paused.
        // Dropping a queued completion strands PlayOnce actions; delaying it
        // can apply an old completion to a newer scripted behavior. Animation
        // advancement remains paused centrally, and attack flags are suppressed.
        if self.stasis.is_some()
            && matches!(
                msg,
                MessagePayload::AnimationFlagTriggered { .. } | MessagePayload::Collided { .. }
            )
        {
            return Effect::NoEffect;
        }
        self.ai.handle_message(entity_id, world, physics, msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::{MessagePayload, ScriptRestoreContext};
    use dark::properties::PropHitPoints;
    use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};

    struct Counter(Rc<Cell<u32>>);
    impl Script for Counter {
        fn update(&mut self, _: EntityId, _: &World, _: &PhysicsWorld, _: &Time) -> Effect {
            self.0.set(self.0.get() + 1);
            Effect::NoEffect
        }
        fn handle_message(
            &mut self,
            _: EntityId,
            _: &World,
            _: &PhysicsWorld,
            _: &MessagePayload,
        ) -> Effect {
            self.0.set(self.0.get() + 1);
            Effect::NoEffect
        }
    }

    #[test]
    fn stasis_pauses_ai_refreshes_expires_and_preserves_remaining_time() {
        let mut world = World::new();
        let entity =
            world.add_entity((PropAI("Melee".to_owned()), PropHitPoints { hit_points: 12 }));
        let count = Rc::new(Cell::new(0));
        let mut script = BaseMonster {
            ai: Box::new(Counter(count.clone())),
            stasis: None,
            hydrated_ai: false,
        };
        let physics = PhysicsWorld::new();
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Freeze {
                duration_seconds: 8.0,
            },
        );
        script.update(
            entity,
            &world,
            &physics,
            &Time {
                elapsed: Duration::from_secs(3),
                total: Duration::from_secs(3),
            },
        );
        assert_eq!(count.get(), 0);
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Collided {
                with: entity,
                contact: None,
            },
        );
        assert_eq!(count.get(), 0, "frozen actors cannot deal contact attacks");
        assert_eq!(script.stasis().unwrap().remaining_seconds, 5.0);
        // A shorter replacement is intentionally shorter, never max/add.
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Freeze {
                duration_seconds: 2.0,
            },
        );
        assert_eq!(script.stasis().unwrap().remaining_seconds, 2.0);
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::AnimationCompleted,
        );
        assert_eq!(count.get(), 1);
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Damage {
                amount: 1.0,
                impact: None,
            },
        );
        assert_eq!(count.get(), 2);
        let saved = script.save_state().unwrap();
        let remap = HashMap::new();
        script.stasis = None;
        script
            .restore_state(
                &saved,
                &ScriptRestoreContext {
                    entity_id_map: &remap,
                },
            )
            .unwrap();
        assert_eq!(script.stasis().unwrap().remaining_seconds, 2.0);
        script.update(
            entity,
            &world,
            &physics,
            &Time {
                elapsed: Duration::from_secs(2),
                total: Duration::from_secs(5),
            },
        );
        assert!(script.stasis().is_none());
        assert_eq!(count.get(), 3, "bookkeeping is retained and AI resumes");
        script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Freeze {
                duration_seconds: 8.0,
            },
        );
        world.add_component(entity, PropHitPoints { hit_points: 0 });
        script.update(entity, &world, &physics, &Time::default());
        assert!(script.stasis().is_none());
        assert_eq!(count.get(), 4, "death update must not wait for expiry");
    }
}
