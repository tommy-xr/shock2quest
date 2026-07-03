use cgmath::Deg;
use dark::motion::MotionQueryItem;
use shipyard::{EntityId, World};

use dark::SCALE_FACTOR;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{
            self, CollisionAvoidanceSteeringStrategy, PathFollowSteeringStrategy, SteeringOutput,
            SteeringStrategy,
        },
    },
    time::Time,
};

use super::Behavior;

/// How far afield a wandering AI will pick destinations (50 Dark feet)
const WANDER_RADIUS: f32 = 50.0 / SCALE_FACTOR;

pub struct WanderBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl WanderBehavior {
    pub fn new() -> WanderBehavior {
        WanderBehavior {
            steering_strategy: steering::chained(vec![
                Box::new(CollisionAvoidanceSteeringStrategy::comprehensive()),
                // Roam to random reachable spots; without AIPATH data this
                // returns None and the AI just walks its current heading
                // (the previous behavior).
                Box::new(PathFollowSteeringStrategy::wander(WANDER_RADIUS)),
            ]),
        }
    }
}

impl Behavior for WanderBehavior {
    fn name(&self) -> &'static str {
        "Wander"
    }

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
