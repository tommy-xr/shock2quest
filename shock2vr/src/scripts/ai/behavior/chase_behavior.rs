use cgmath::Deg;
use dark::motion::MotionQueryItem;
use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{
            self, ChasePlayerSteeringStrategy, CollisionAvoidanceSteeringStrategy,
            PathFollowSteeringStrategy, SteeringOutput, SteeringStrategy,
        },
    },
    time::Time,
};

use super::{Behavior, NextBehavior};

pub struct ChaseBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl ChaseBehavior {
    pub fn new() -> ChaseBehavior {
        ChaseBehavior {
            steering_strategy: steering::chained(vec![
                // Path steering leads: the route already avoids static
                // geometry (nav-mesh + edge clearance), and letting whisker
                // avoidance preempt it deadlocks AIs against walls the path
                // was about to turn away from (issue #481). Avoidance guards
                // only the direct-chase fallback below.
                Box::new(PathFollowSteeringStrategy::chase_player()),
                Box::new(
                    CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
                ),
                Box::new(ChasePlayerSteeringStrategy),
            ]),
        }
    }
}

impl Behavior for ChaseBehavior {
    fn name(&self) -> &'static str {
        "Chase"
    }

    fn turn_speed(&self) -> Deg<f32> {
        Deg(360.0)
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

    fn animation(self: &ChaseBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn is_locomotion(&self) -> bool {
        true
    }

    fn next_behavior(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        match super::attack_behavior_for_distance(world, physics, entity_id) {
            Some(behavior) => NextBehavior::Next(behavior),
            None => NextBehavior::Stay,
        }
    }
}
