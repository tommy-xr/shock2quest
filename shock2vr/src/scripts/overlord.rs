use dark::properties::{AIMode, Link, Links};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, IntoIter, IntoWithId, View, World};

use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    effect::AIPropertyUpdate, script_util,
};
use crate::{physics::PhysicsWorld, time::Time};

const STATE_KEY: &str = "shock2vr.overlord";

/// Retail SlayResult=1 keeps the projection itself alive in the object system.
/// Slain schedules Materialize at 30 s; ten 100 ms Out ticks fade it, then
/// HasRefs and AI mode hide it. BrainDead changes future deaths to normal.
#[derive(Default, Serialize, Deserialize)]
pub struct Overlord {
    dematerialized: bool,
    return_in: Option<f32>,
    out_in: Option<f32>,
    alpha_steps: u8,
}

impl Overlord {
    pub fn new() -> Self {
        Self::default()
    }

    fn sound(world: &World, entity: EntityId, event: &str) -> Effect {
        script_util::play_environmental_sound(world, entity, event, vec![], AudioHandle::new())
    }

    fn materialize(&mut self, world: &World, entity: EntityId) -> Effect {
        self.dematerialized = false;
        self.out_in = None;
        self.alpha_steps = 10;
        Effect::combine(vec![
            Self::sound(world, entity, "activate"),
            Effect::ResurrectEntity { entity_id: entity },
            Effect::SetMetaProperty {
                entity_id: entity,
                name: "Dematerialized".into(),
                add: false,
            },
            Effect::SetWorldPresence {
                entity_id: entity,
                present: true,
            },
            Effect::SetAIProperty {
                entity_id: entity,
                update: AIPropertyUpdate::Mode {
                    mode: AIMode::Normal,
                },
            },
            // The reviewed retail In timer restores alpha on its first tick;
            // it does not implement a symmetric one-second fade-in.
            Effect::SetRenderAlpha {
                entity_id: entity,
                alpha: 1.0,
            },
        ])
    }
}

impl Script for Overlord {
    fn handle_message(
        &mut self,
        entity: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Slay
                if script_util::retain_on_slay(world, entity) && !self.dematerialized =>
            {
                self.dematerialized = true;
                self.return_in = Some(30.0);
                self.out_in = Some(0.0);
                self.alpha_steps = 10;
                Effect::combine(vec![
                    Self::sound(world, entity, "deactivate"),
                    Effect::SetMetaProperty {
                        entity_id: entity,
                        name: "Dematerialized".into(),
                        add: true,
                    },
                    Effect::SetRenderAlpha {
                        entity_id: entity,
                        alpha: 1.0,
                    },
                ])
            }
            MessagePayload::BrainDead => {
                let materialize = if self.dematerialized {
                    self.materialize(world, entity)
                } else {
                    Effect::NoEffect
                };
                // Retail does not cancel an already scheduled Materialize.
                Effect::combine(vec![
                    materialize,
                    Effect::SetSlayResult {
                        entity_id: entity,
                        result: 0,
                    },
                ])
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(&mut self, entity: EntityId, world: &World, _: &PhysicsWorld, time: &Time) -> Effect {
        let dt = time.elapsed.as_secs_f32();
        if let Some(remaining) = &mut self.return_in {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.return_in = None;
                return self.materialize(world, entity);
            }
        }
        let mut effects = Vec::new();
        if let Some(mut next) = self.out_in {
            next -= dt;
            while next <= 0.0 {
                if self.alpha_steps == 0 {
                    self.out_in = None;
                    effects.push(Effect::SetWorldPresence {
                        entity_id: entity,
                        present: false,
                    });
                    effects.push(Effect::SetAIProperty {
                        entity_id: entity,
                        update: AIPropertyUpdate::Mode {
                            mode: AIMode::Asleep,
                        },
                    });
                    return Effect::combine(effects);
                }
                self.alpha_steps -= 1;
                effects.push(Effect::SetRenderAlpha {
                    entity_id: entity,
                    alpha: self.alpha_steps as f32 / 10.0,
                });
                next += 0.1;
            }
            self.out_in = Some(next);
        }
        Effect::combine(effects)
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }
    fn restore_state(
        &mut self,
        saved: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = saved.decode(1, STATE_KEY)?;
        if self.alpha_steps > 10
            || self.return_in.is_some_and(|v| !v.is_finite() || v < 0.0)
            || self.out_in.is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err(ScriptStateError::InvalidPayload {
                script_key: STATE_KEY.into(),
                message: "invalid materialization timer".into(),
            });
        }
        Ok(())
    }
}

pub struct Brain;
impl Script for Brain {
    fn handle_message(
        &mut self,
        entity: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Slay) {
            return Effect::NoEffect;
        }
        let links = world.borrow::<View<Links>>().unwrap();
        Effect::combine(
            (&links)
                .iter()
                .with_id()
                .flat_map(|(source, links)| {
                    links.to_links.iter().filter_map(move |link| {
                        (matches!(link.link, Link::AIWatchObj(_))
                            && link.to_entity_id.is_some_and(|id| id.0 == entity))
                        .then_some(Effect::Send {
                            msg: Message {
                                to: source,
                                payload: MessagePayload::BrainDead,
                            },
                        })
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{AIWatchOptions, PropSlayResult, ToLink, WrappedEntityId};
    use std::{collections::HashMap, time::Duration};

    fn tick(script: &mut Overlord, world: &World, entity: EntityId, seconds: f32) -> Vec<Effect> {
        Effect::flatten(vec![script.update(
            entity,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs_f32(seconds),
                total: Duration::ZERO,
            },
        )])
    }
    fn slay(script: &mut Overlord, world: &World, entity: EntityId) -> Vec<Effect> {
        Effect::flatten(vec![script.handle_message(
            entity,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Slay,
        )])
    }

    #[test]
    fn projection_fades_hides_then_returns_after_thirty_seconds_across_saved_states() {
        let mut world = World::new();
        let entity = world.add_entity(PropSlayResult(1));
        for save_at in [0.35, 4.0, 29.0] {
            let mut projection = Overlord::new();
            assert!(slay(&mut projection, &world, entity).iter().any(|e| matches!(e, Effect::SetMetaProperty { add: true, name, .. } if name == "Dematerialized")));
            let faded = tick(&mut projection, &world, entity, save_at);
            if save_at > 1.0 {
                assert!(
                    faded
                        .iter()
                        .any(|e| matches!(e, Effect::SetWorldPresence { present: false, .. }))
                );
            } else {
                assert!(faded.iter().any(|e| matches!(e, Effect::SetRenderAlpha { alpha, .. } if *alpha > 0.0 && *alpha < 1.0)));
            }
            assert!(
                slay(&mut projection, &world, entity).is_empty(),
                "duplicate Slain cannot restart the timer"
            );
            let saved = projection.save_state().unwrap();
            let mut restored = Overlord::new();
            restored
                .restore_state(&saved, &ScriptRestoreContext::new(&HashMap::new()))
                .unwrap();
            assert!(
                !tick(&mut restored, &world, entity, 29.5 - save_at)
                    .iter()
                    .any(|e| matches!(e, Effect::ResurrectEntity { .. }))
            );
            let returned = tick(&mut restored, &world, entity, 0.6);
            assert_eq!(returned.iter().filter(|e| matches!(e, Effect::ResurrectEntity { entity_id } if *entity_id == entity)).count(), 1);
            assert!(
                returned
                    .iter()
                    .any(|e| matches!(e, Effect::SetWorldPresence { present: true, .. }))
            );
            assert!(tick(&mut restored, &world, entity, 60.0).is_empty());
        }
    }

    #[test]
    fn brain_dead_preserves_living_projection_but_returns_hidden_projection_as_mortal() {
        let mut world = World::new();
        let entity = world.add_entity(PropSlayResult(1));
        for slain in [false, true] {
            let mut projection = Overlord::new();
            if slain {
                slay(&mut projection, &world, entity);
                tick(&mut projection, &world, entity, 2.0);
            }
            let effects = Effect::flatten(vec![projection.handle_message(
                entity,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::BrainDead,
            )]);
            assert_eq!(
                effects
                    .iter()
                    .any(|e| matches!(e, Effect::ResurrectEntity { .. })),
                slain
            );
            assert!(
                effects
                    .iter()
                    .any(|e| matches!(e, Effect::SetSlayResult { result: 0, .. }))
            );
            assert!(effects.iter().all(|e| !matches!(
                e,
                Effect::DestroyEntity { .. }
                    | Effect::SlayEntity { .. }
                    | Effect::GenerateLoot { .. }
            )));
        }
        world.add_component(entity, PropSlayResult(0));
        assert!(slay(&mut Overlord::new(), &world, entity).is_empty());
    }

    #[test]
    fn brain_sends_brain_dead_only_to_incoming_watchers() {
        let mut world = World::new();
        let brain = world.add_entity(());
        let other = world.add_entity(());
        let watcher = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 1,
                to_entity_id: Some(WrappedEntityId(brain)),
                link: Link::AIWatchObj(AIWatchOptions {
                    radius: 0.0,
                    height: 0.0,
                    scripted_actions: vec![],
                }),
            }],
        });
        world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 2,
                to_entity_id: Some(WrappedEntityId(other)),
                link: Link::SwitchLink,
            }],
        });
        let effects = Effect::flatten(vec![Brain.handle_message(
            brain,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Slay,
        )]);
        assert_eq!(effects.len(), 1);
        assert!(
            matches!(&effects[0], Effect::Send { msg } if msg.to == watcher && matches!(msg.payload, MessagePayload::BrainDead))
        );
    }

    #[test]
    fn script_dispatch_retains_projections_but_still_terminates_normal_slays() {
        use crate::scripts::ScriptWorld;
        let mut world = World::new();
        let retained = world.add_entity(PropSlayResult(1));
        let normal = world.add_entity(PropSlayResult(0));
        let mut scripts = ScriptWorld::new();
        scripts.add_entity2(retained, Box::new(Overlord::new()));
        scripts.add_entity2(normal, Box::new(Overlord::new()));
        for entity in [retained, normal] {
            scripts.dispatch(Message {
                to: entity,
                payload: MessagePayload::Slay,
            });
        }
        let effects = scripts.update(&world, &PhysicsWorld::new(), &Time::default());
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::SlayEntity { entity_id } if *entity_id == retained))
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SlayEntity { entity_id } if *entity_id == normal))
        );
    }
    #[test]
    fn inline_lethal_dispatch_also_retains_the_projection() {
        use crate::scripts::{ScriptWorld, internal_simple_health::InternalSimpleHealth};
        let mut world = World::new();
        let entity = world.add_entity((
            PropSlayResult(1),
            dark::properties::PropHitPoints { hit_points: 1 },
        ));
        let mut scripts = ScriptWorld::new();
        scripts.add_entity2(entity, Box::new(InternalSimpleHealth::new()));
        scripts.add_entity2(entity, Box::new(Overlord::new()));
        scripts.dispatch(Message {
            to: entity,
            payload: MessagePayload::Damage {
                amount: 10.0,
                impact: None,
            },
        });
        let effects = scripts.update(&world, &PhysicsWorld::new(), &Time::default());
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SetMetaProperty { add: true, .. }))
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::SlayEntity { .. } | Effect::GenerateLoot { .. }))
        );
    }
}
