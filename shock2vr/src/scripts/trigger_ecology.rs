use dark::properties::{PropEcoState, PropEcoType, PropEcology, PropHitPoints};
use rand::Rng;
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, ViewMut, World};

use crate::{
    mission::mission_core::GlobalTemplateHierarchy, physics::PhysicsWorld,
    runtime_props::RuntimePropEcologyState, time::Time,
};

use super::{
    Effect, MessagePayload, Script,
    script_util::{entity_class_template_id, send_to_all_switch_links},
};

const PHYSICAL_TEMPLATE_ID: i32 = -11;
const ECOLOGY_STATE_NORMAL: i32 = 0;
const ECOLOGY_STATE_HACKED: i32 = 1;
const ECOLOGY_STATE_ALERT: i32 = 2;

/// Retail `TriggerEcology`: periodically count live physical objects carrying
/// this trigger's EcoType and pulse its SwitchLinks when population is low.
pub struct TriggerEcology;

impl TriggerEcology {
    pub fn new() -> Self {
        Self
    }

    fn population(world: &World, ecology_type: i32) -> usize {
        let Ok(ecology_types) = world.borrow::<View<PropEcoType>>() else {
            return 0;
        };
        let Ok(hierarchy) = world.borrow::<UniqueView<GlobalTemplateHierarchy>>() else {
            return 0;
        };
        let hit_points = world.borrow::<View<PropHitPoints>>().ok();
        ecology_types
            .iter()
            .with_id()
            .filter(|(entity, candidate)| {
                candidate.0 == ecology_type
                    && hit_points
                        .as_ref()
                        .and_then(|hit_points| hit_points.get(*entity).ok())
                        .is_none_or(|hit_points| hit_points.hit_points > 0)
                    && entity_class_template_id(world, *entity).is_some_and(|template| {
                        hierarchy.is_or_descends_from(template, PHYSICAL_TEMPLATE_ID)
                    })
            })
            .count()
    }

    fn should_spawn(population: i32, minimum: i32, maximum: i32, random_hit: bool) -> bool {
        population < maximum && (population < minimum || random_hit)
    }
}

impl Script for TriggerEcology {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let elapsed = time.elapsed.as_secs_f32();
        let period_seconds = world
            .borrow::<View<PropEcology>>()
            .ok()
            .and_then(|ecologies| {
                ecologies
                    .get(entity_id)
                    .ok()
                    .map(|ecology| ecology.period_seconds.max(0.0))
            });
        let Some(period_seconds) = period_seconds else {
            return Effect::NoEffect;
        };

        let (should_poll, recovery_expired) = {
            let mut runtime_states = world.borrow::<ViewMut<RuntimePropEcologyState>>().unwrap();
            let mut ecology_states = world.borrow::<ViewMut<PropEcoState>>().unwrap();
            let (Ok(runtime_state), Ok(ecology_state)) = (
                (&mut runtime_states).get(entity_id),
                (&mut ecology_states).get(entity_id),
            ) else {
                return Effect::NoEffect;
            };

            let mut recovery_expired = false;
            if ecology_state.0 == ECOLOGY_STATE_ALERT
                && let Some(remaining) = &mut runtime_state.recovery_seconds_remaining
            {
                *remaining -= elapsed;
                if *remaining <= 0.0 {
                    ecology_state.0 = ECOLOGY_STATE_NORMAL;
                    runtime_state.recovery_seconds_remaining = None;
                    recovery_expired = true;
                }
            }

            runtime_state.seconds_until_poll -= elapsed;
            let should_poll = runtime_state.seconds_until_poll <= 0.0;
            if should_poll {
                runtime_state.seconds_until_poll = period_seconds;
            }
            (should_poll, recovery_expired)
        };

        if recovery_expired {
            return send_to_all_switch_links(
                world,
                entity_id,
                MessagePayload::Reset { from: entity_id },
            );
        }
        if !should_poll {
            return Effect::NoEffect;
        }

        let state = world
            .borrow::<View<PropEcoState>>()
            .ok()
            .and_then(|states| states.get(entity_id).ok().map(|state| state.0))
            .unwrap_or(ECOLOGY_STATE_NORMAL);
        let state_index = match state {
            ECOLOGY_STATE_NORMAL => ECOLOGY_STATE_NORMAL as usize,
            ECOLOGY_STATE_ALERT => ECOLOGY_STATE_ALERT as usize,
            ECOLOGY_STATE_HACKED => return Effect::NoEffect,
            _ => return Effect::NoEffect,
        };
        let ecologies = world.borrow::<View<PropEcology>>().unwrap();
        let ecology_types = world.borrow::<View<PropEcoType>>().unwrap();
        let Ok(ecology) = ecologies.get(entity_id) else {
            return Effect::NoEffect;
        };
        let Ok(ecology_type) = ecology_types.get(entity_id) else {
            return Effect::NoEffect;
        };
        let minimum = ecology.min_count[state_index];
        let maximum = ecology.max_count[state_index];
        let random_chance = ecology.random_chance[state_index];
        let population = Self::population(world, ecology_type.0) as i32;
        let random_hit = random_chance > 0 && rand::thread_rng().gen_range(0..random_chance) == 0;
        if Self::should_spawn(population, minimum, maximum, random_hit) {
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id })
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Alarm { victim, .. } => {
                let recovery_seconds =
                    world
                        .borrow::<View<PropEcology>>()
                        .ok()
                        .and_then(|ecologies| {
                            ecologies.get(entity_id).ok().map(|ecology| {
                                ecology.recovery_seconds[ECOLOGY_STATE_ALERT as usize]
                            })
                        });
                let Some(recovery_seconds) = recovery_seconds else {
                    return Effect::NoEffect;
                };

                let transitioned = {
                    let mut ecology_states = world.borrow::<ViewMut<PropEcoState>>().unwrap();
                    let mut runtime_states =
                        world.borrow::<ViewMut<RuntimePropEcologyState>>().unwrap();
                    let (Ok(ecology_state), Ok(runtime_state)) = (
                        (&mut ecology_states).get(entity_id),
                        (&mut runtime_states).get(entity_id),
                    ) else {
                        return Effect::NoEffect;
                    };
                    if ecology_state.0 != ECOLOGY_STATE_NORMAL {
                        false
                    } else {
                        ecology_state.0 = ECOLOGY_STATE_ALERT;
                        runtime_state.recovery_seconds_remaining = Some(recovery_seconds.max(0.0));
                        true
                    }
                };

                if transitioned {
                    send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::Alarm {
                            from: entity_id,
                            victim: *victim,
                        },
                    )
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Reset { .. } => {
                let transitioned = {
                    let mut ecology_states = world.borrow::<ViewMut<PropEcoState>>().unwrap();
                    let mut runtime_states =
                        world.borrow::<ViewMut<RuntimePropEcologyState>>().unwrap();
                    let (Ok(ecology_state), Ok(runtime_state)) = (
                        (&mut ecology_states).get(entity_id),
                        (&mut runtime_states).get(entity_id),
                    ) else {
                        return Effect::NoEffect;
                    };
                    if ecology_state.0 != ECOLOGY_STATE_ALERT {
                        false
                    } else {
                        ecology_state.0 = ECOLOGY_STATE_NORMAL;
                        runtime_state.recovery_seconds_remaining = None;
                        true
                    }
                };

                if transitioned {
                    send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::Reset { from: entity_id },
                    )
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use dark::properties::{Link, Links, PropTemplateId, ToLink, WrappedEntityId};

    use super::*;
    use crate::runtime_props::RuntimePropCanonicalTemplateId;

    #[test]
    fn pulses_when_matching_physical_population_is_below_minimum() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::from([(
            -196,
            vec![PHYSICAL_TEMPLATE_ID],
        )])));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            PropEcology {
                period_seconds: 15.0,
                min_count: [1, 0, 0],
                max_count: [1, 0, 0],
                recovery_seconds: [0.0; 3],
                random_chance: [1, 0, 0],
            },
            RuntimePropEcologyState::new(15.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        let effect = script.update(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(15),
                total: Duration::from_secs(15),
            },
        );
        assert!(matches!(
            effect,
            Effect::Combined { effects }
                if effects.iter().any(|effect| matches!(
                    effect,
                    Effect::Send { msg }
                        if msg.to == generator
                ))
        ));

        world.add_entity((PropEcoType(2501), RuntimePropCanonicalTemplateId(-196)));
        let effect = script.update(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(15),
                total: Duration::from_secs(30),
            },
        );
        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn hacked_ecology_is_paused_even_with_nonzero_population_targets() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(1),
            PropEcology {
                period_seconds: 15.0,
                min_count: [0, 1, 0],
                max_count: [0, 1, 0],
                recovery_seconds: [0.0; 3],
                random_chance: [0, 1, 0],
            },
            RuntimePropEcologyState::new(15.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();
        script.initialize(ecology, &world);

        let effect = script.update(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(15),
                total: Duration::from_secs(15),
            },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn alarm_selects_alert_profile_and_starts_authored_recovery_once() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let victim = world.add_entity(());
        let generator = world.add_entity(());
        let camera = world.add_entity(());
        let ecology = world.add_entity((
            PropTemplateId { template_id: 292 },
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_NORMAL),
            PropEcology {
                period_seconds: 15.0,
                min_count: [0, 0, 2],
                max_count: [0, 0, 2],
                recovery_seconds: [0.0, 0.0, 120.0],
                random_chance: [0, 0, 0],
            },
            RuntimePropEcologyState::new(15.0),
            Links {
                to_links: vec![
                    ToLink {
                        to_template_id: 293,
                        to_entity_id: Some(WrappedEntityId(generator)),
                        link: Link::SwitchLink,
                    },
                    ToLink {
                        to_template_id: 294,
                        to_entity_id: Some(WrappedEntityId(camera)),
                        link: Link::SwitchLink,
                    },
                ],
            },
        ));
        let mut script = TriggerEcology::new();

        let effect = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm {
                from: camera,
                victim,
            },
        );

        let states = world.borrow::<View<PropEcoState>>().unwrap();
        assert_eq!(states.get(ecology).unwrap().0, ECOLOGY_STATE_ALERT);
        drop(states);
        let runtime_states = world.borrow::<View<RuntimePropEcologyState>>().unwrap();
        assert_eq!(
            runtime_states
                .get(ecology)
                .unwrap()
                .recovery_seconds_remaining,
            Some(120.0)
        );
        drop(runtime_states);
        let sent_to = Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg }
                    if matches!(
                        msg.payload,
                        MessagePayload::Alarm {
                            victim: sent_victim,
                            ..
                        } if sent_victim == victim
                    ) =>
                {
                    Some(msg.to)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sent_to, vec![generator, camera]);

        {
            let mut runtime_states = world.borrow::<ViewMut<RuntimePropEcologyState>>().unwrap();
            (&mut runtime_states)
                .get(ecology)
                .unwrap()
                .recovery_seconds_remaining = Some(17.0);
        }
        let repeated = script.handle_message(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Alarm {
                from: camera,
                victim,
            },
        );
        assert!(matches!(repeated, Effect::NoEffect));
        let runtime_states = world.borrow::<View<RuntimePropEcologyState>>().unwrap();
        assert_eq!(
            runtime_states
                .get(ecology)
                .unwrap()
                .recovery_seconds_remaining,
            Some(17.0),
            "repeated Alarm must not extend the retail recovery timer"
        );
    }

    #[test]
    fn recovery_expiry_resets_state_and_linked_security_devices() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let camera = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            PropEcology {
                period_seconds: 15.0,
                min_count: [0, 0, 2],
                max_count: [0, 0, 2],
                recovery_seconds: [0.0, 0.0, 120.0],
                random_chance: [0, 0, 0],
            },
            RuntimePropEcologyState {
                seconds_until_poll: 10.0,
                recovery_seconds_remaining: Some(1.0),
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 294,
                    to_entity_id: Some(WrappedEntityId(camera)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();

        let effect = script.update(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(1),
                total: Duration::from_secs(1),
            },
        );

        let states = world.borrow::<View<PropEcoState>>().unwrap();
        assert_eq!(states.get(ecology).unwrap().0, ECOLOGY_STATE_NORMAL);
        assert!(Effect::flatten(vec![effect]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == camera
                        && matches!(msg.payload, MessagePayload::Reset { from } if from == ecology)
            )
        }));
    }

    #[test]
    fn alert_profile_pulses_until_its_population_minimum_is_met() {
        let mut world = World::new();
        world.add_unique(GlobalTemplateHierarchy(HashMap::new()));
        let generator = world.add_entity(());
        let ecology = world.add_entity((
            PropEcoType(2501),
            PropEcoState(ECOLOGY_STATE_ALERT),
            PropEcology {
                period_seconds: 15.0,
                min_count: [0, 0, 2],
                max_count: [0, 0, 2],
                recovery_seconds: [0.0, 0.0, 120.0],
                random_chance: [0, 0, 0],
            },
            RuntimePropEcologyState {
                seconds_until_poll: 15.0,
                recovery_seconds_remaining: Some(120.0),
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 293,
                    to_entity_id: Some(WrappedEntityId(generator)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let mut script = TriggerEcology::new();

        let effect = script.update(
            ecology,
            &world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs(15),
                total: Duration::from_secs(15),
            },
        );

        assert!(
            Effect::flatten(vec![effect])
                .into_iter()
                .any(|effect| { matches!(effect, Effect::Send { msg } if msg.to == generator) })
        );
    }

    #[test]
    fn random_spawn_never_exceeds_maximum() {
        assert!(TriggerEcology::should_spawn(0, 1, 2, false));
        assert!(TriggerEcology::should_spawn(1, 1, 2, true));
        assert!(!TriggerEcology::should_spawn(1, 1, 2, false));
        assert!(!TriggerEcology::should_spawn(2, 1, 2, true));
    }
}
