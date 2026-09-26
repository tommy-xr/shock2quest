//! Awareness shared by actors whose motion is not driven by skeletal clips.
use super::{
    ai_util,
    alertness::{self, AlertnessState, AlertnessTimings},
};
use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    scripts::{AIPropertyUpdate, Effect},
};
use cgmath::Vector3;
use dark::properties::{AIAlertLevel, PropAIAlertCap, PropAIAlertness, PropAIAwareDelay};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct MobileAwareness {
    pub alertness: AlertnessState,
    pub target: Option<Vector3<f32>>,
    pub pinned: bool,
}
impl MobileAwareness {
    fn cap(world: &World, entity: EntityId) -> PropAIAlertCap {
        world
            .borrow::<View<PropAIAlertCap>>()
            .unwrap()
            .get(entity)
            .cloned()
            .unwrap_or(PropAIAlertCap {
                min_level: AIAlertLevel::Lowest,
                max_level: AIAlertLevel::High,
                min_relax: AIAlertLevel::Low,
            })
    }
    pub fn set_alertness(
        &mut self,
        world: &World,
        entity: EntityId,
        level: AIAlertLevel,
        pinned: bool,
    ) -> Effect {
        self.pinned = pinned;
        alertness::set_level(&mut self.alertness, level, &Self::cap(world, entity));
        if self.alertness.current_level == AIAlertLevel::Lowest {
            self.target = None;
        }
        alertness::sync_alertness_effect(entity, &self.alertness)
    }
    pub fn hear(&mut self, world: &World, entity: EntityId, origin: Vector3<f32>) -> Effect {
        self.target = Some(origin);
        alertness::set_level(
            &mut self.alertness,
            AIAlertLevel::High,
            &Self::cap(world, entity),
        );
        Effect::combine(vec![
            alertness::sync_alertness_effect(entity, &self.alertness),
            Effect::SetAIProperty {
                entity_id: entity,
                update: AIPropertyUpdate::TargetAwareness {
                    last_known_pos: origin,
                    has_line_of_sight: false,
                },
            },
        ])
    }
    pub fn from_world(world: &World, entity: EntityId) -> Self {
        let mut state = Self::default();
        if let Ok(prop) = world.borrow::<View<PropAIAlertness>>().unwrap().get(entity) {
            state.alertness.current_level = prop.level;
            state.alertness.peak_level = prop.peak;
        }
        state
    }
    pub fn update(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity: EntityId,
        dt: f32,
    ) -> (bool, Effect) {
        let visible = self.pinned || ai_util::is_player_visible(entity, world, physics);
        let cap = Self::cap(world, entity);
        let timings = world
            .borrow::<View<PropAIAwareDelay>>()
            .unwrap()
            .get(entity)
            .map(AlertnessTimings::from_aware_delay)
            .unwrap_or_default();
        if !self.pinned {
            alertness::process_alertness_update(&mut self.alertness, visible, dt, &timings, &cap);
        }
        if visible {
            self.target = world.borrow::<UniqueView<PlayerInfo>>().ok().map(|p| p.pos);
        } else if self.alertness.current_level == AIAlertLevel::Lowest {
            self.target = None;
        }
        let awareness = match self.target {
            Some(pos) => AIPropertyUpdate::TargetAwareness {
                last_known_pos: pos,
                has_line_of_sight: visible,
            },
            None => AIPropertyUpdate::ClearTargetAwareness,
        };
        (
            visible,
            Effect::combine(vec![
                alertness::sync_alertness_effect(entity, &self.alertness),
                Effect::SetAIProperty {
                    entity_id: entity,
                    update: awareness,
                },
            ]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_retains_its_origin_and_forced_alertness_respects_authored_caps() {
        let mut world = World::new();
        let entity = world.add_entity((PropAIAlertCap {
            min_level: AIAlertLevel::Lowest,
            max_level: AIAlertLevel::Moderate,
            min_relax: AIAlertLevel::Lowest,
        },));
        let mut state = MobileAwareness::default();
        let origin = cgmath::vec3(2.0, 3.0, 4.0);
        state.hear(&world, entity, origin);
        assert_eq!(state.target, Some(origin));
        assert_eq!(state.alertness.current_level, AIAlertLevel::Moderate);
        state.set_alertness(&world, entity, AIAlertLevel::High, true);
        assert_eq!(state.alertness.current_level, AIAlertLevel::Moderate);
        assert!(state.pinned);
        state.set_alertness(&world, entity, AIAlertLevel::Lowest, false);
        assert_eq!(state.target, None);
        assert!(!state.pinned);
    }
}
