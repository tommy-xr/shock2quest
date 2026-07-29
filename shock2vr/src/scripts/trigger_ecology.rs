use dark::properties::{PropEcoState, PropEcoType, PropEcology, PropHitPoints};
use rand::Rng;
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{mission::mission_core::GlobalTemplateHierarchy, physics::PhysicsWorld, time::Time};

use super::{
    Effect, Script,
    script_util::{entity_class_template_id, send_to_all_switch_links},
};

const PHYSICAL_TEMPLATE_ID: i32 = -11;

/// Retail `TriggerEcology`: periodically count live physical objects carrying
/// this trigger's EcoType and pulse its SwitchLinks when population is low.
pub struct TriggerEcology {
    seconds_until_poll: f32,
}

impl TriggerEcology {
    pub fn new() -> Self {
        Self {
            seconds_until_poll: 0.0,
        }
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
}

impl Script for TriggerEcology {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let ecologies = world.borrow::<View<PropEcology>>().unwrap();
        self.seconds_until_poll = ecologies
            .get(entity_id)
            .map(|ecology| ecology.period_seconds.max(0.0))
            .unwrap_or(0.0);
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.seconds_until_poll -= time.elapsed.as_secs_f32();
        if self.seconds_until_poll > 0.0 {
            return Effect::NoEffect;
        }

        let ecologies = world.borrow::<View<PropEcology>>().unwrap();
        let ecology_types = world.borrow::<View<PropEcoType>>().unwrap();
        let Ok(ecology) = ecologies.get(entity_id) else {
            return Effect::NoEffect;
        };
        let Ok(ecology_type) = ecology_types.get(entity_id) else {
            return Effect::NoEffect;
        };
        self.seconds_until_poll = ecology.period_seconds.max(0.0);

        let state = world
            .borrow::<View<PropEcoState>>()
            .ok()
            .and_then(|states| states.get(entity_id).ok().map(|state| state.0))
            .unwrap_or(0);
        let Ok(state_index) = usize::try_from(state) else {
            return Effect::NoEffect;
        };
        let (Some(&minimum), Some(&maximum), Some(&random_chance)) = (
            ecology.min_count.get(state_index),
            ecology.max_count.get(state_index),
            ecology.random_chance.get(state_index),
        ) else {
            return Effect::NoEffect;
        };
        let population = Self::population(world, ecology_type.0) as i32;
        let random_hit = random_chance > 0 && rand::thread_rng().gen_range(0..random_chance) == 0;
        if population < maximum && (population < minimum || random_hit) {
            send_to_all_switch_links(
                world,
                entity_id,
                super::MessagePayload::TurnOn { from: entity_id },
            )
        } else {
            Effect::NoEffect
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
            PropEcology {
                period_seconds: 15.0,
                min_count: [1, 0, 0],
                max_count: [1, 0, 0],
                recovery_seconds: [0.0; 3],
                random_chance: [1, 0, 0],
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
}
