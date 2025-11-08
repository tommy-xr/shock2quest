use cgmath::Deg;
use dark::motion::MotionQueryItem;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{CollisionAvoidanceSteeringStrategy, SteeringOutput, SteeringStrategy},
    },
    time::Time,
};

use super::Behavior;

pub struct WanderBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl WanderBehavior {
    pub fn new() -> WanderBehavior {
        WanderBehavior {
            steering_strategy: Box::new(CollisionAvoidanceSteeringStrategy::comprehensive()),
        }
    }
}

impl Behavior for WanderBehavior {
    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn animation(self: &WanderBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::with_value("direction", 0).optional(),
            MotionQueryItem::new("locomote"),
            //MotionQueryItem::new("search").optional(),
        ]
    }

    fn is_locomotion(&self) -> bool {
        true
    }
}
