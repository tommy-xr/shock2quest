use cgmath::Deg;

use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{ai::ai_util::random_binomial, Effect},
    time::Time,
};

use super::SteeringOutput;

#[allow(dead_code)]
pub struct WanderSteeringStrategy {
    maybe_current_heading: Option<Deg<f32>>,
}

impl WanderSteeringStrategy {
    #[allow(dead_code)]
    pub fn new() -> WanderSteeringStrategy {
        WanderSteeringStrategy {
            maybe_current_heading: None,
        }
    }

    #[allow(dead_code)]
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
        Some((
            SteeringOutput {
                desired_heading: self.maybe_current_heading.unwrap(),
            },
            Effect::NoEffect,
        ))
    }
}
