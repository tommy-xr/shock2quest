use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3,
};
use dark::{properties::PropPosition, SCALE_FACTOR};

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{ai::ai_util::random_binomial, Effect},
    time::Time,
    util::{get_position_from_transform, get_rotation_from_transform, vec3_to_point3},
};

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
