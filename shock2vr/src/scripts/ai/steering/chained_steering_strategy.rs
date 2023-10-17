use cgmath::Deg;

use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, scripts::Effect, time::Time};

use super::{SteeringOutput, SteeringStrategy};

pub struct ChainedSteeringStrategy {
    strategies: Vec<Box<dyn SteeringStrategy>>,
}

impl SteeringStrategy for ChainedSteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        for strategy in &mut self.strategies {
            if let Some((steering_output, effect)) =
                strategy.steer(_current_heading, _world, _physics, _entity_id, _time)
            {
                return Some((steering_output, effect));
            }
        }

        None
    }
}

pub fn chained(strategies: Vec<Box<dyn SteeringStrategy>>) -> Box<dyn SteeringStrategy> {
    Box::new(ChainedSteeringStrategy { strategies })
}
