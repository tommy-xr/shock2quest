mod chained_steering_strategy;
mod chase_entity_steering_strategy;
mod chase_player_steering_strategy;
mod collision_avoidance_steering_strategy;
mod wander_steering_strategy;

pub use chained_steering_strategy::*;
pub use chase_entity_steering_strategy::*;
pub use chase_player_steering_strategy::*;
pub use collision_avoidance_steering_strategy::*;

use cgmath::{Deg, EuclideanSpace, Point3};
use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect,
    ai_util::{self},
};

pub struct SteeringOutput {
    pub desired_heading: Deg<f32>,
}

impl Default for SteeringOutput {
    fn default() -> Self {
        SteeringOutput {
            desired_heading: Deg(0.0),
        }
    }
}
pub struct Steering;

impl Steering {
    pub fn from_current(heading: Deg<f32>) -> SteeringOutput {
        SteeringOutput {
            desired_heading: heading,
        }
    }

    pub fn turn_to_point(position: Point3<f32>, target: Point3<f32>) -> SteeringOutput {
        let yaw = ai_util::yaw_between_vectors(position.to_vec(), target.to_vec());
        SteeringOutput {
            desired_heading: yaw,
        }
    }
}

pub trait SteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        None
    }
}
