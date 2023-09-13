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

pub struct WanderSteeringStrategy {
    maybe_current_heading: Option<Deg<f32>>,
}

impl WanderSteeringStrategy {
    pub fn new() -> WanderSteeringStrategy {
        WanderSteeringStrategy {
            maybe_current_heading: None,
        }
    }

    pub fn steer(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        if let Some(current_heading) = self.maybe_current_heading {
            self.maybe_current_heading = Some(Deg(
                current_heading.0 + 100.0 * random_binomial() * time.elapsed.as_secs_f32()
            ))
        } else {
            self.maybe_current_heading = Some(current_heading);
        };
        println!("steering output: {:?}", self.maybe_current_heading);
        Some((
            SteeringOutput {
                desired_heading: self.maybe_current_heading.unwrap(),
            },
            Effect::NoEffect,
        ))
    }
}
