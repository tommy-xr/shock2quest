use std::cell::RefCell;

use cgmath::{Deg, Vector3};
use dark::SCALE_FACTOR;
use dark::motion::MotionQueryItem;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::ai_util,
        ai::steering::{
            self, CollisionAvoidanceSteeringStrategy, PathFollowSteeringStrategy, SteeringOutput,
            SteeringStrategy,
        },
    },
    time::Time,
};

use super::{Behavior, NextBehavior, WanderBehavior};

/// Close enough to the last-known position to stop and look around
const SEARCH_ARRIVE_DISTANCE: f32 = 4.0 / SCALE_FACTOR;
/// How long to scan around the last-known position before giving up
const SEARCH_SCAN_SECONDS: f32 = 6.0;
/// Give up entirely after this long, arrived or not (unreachable positions,
/// blocked routes)
const SEARCH_MAX_SECONDS: f32 = 20.0;

/// Investigate the target's last-known position: walk there via the nav
/// mesh, scan around for a few seconds, then hand off to Wander. Alertness
/// escalation (the player becoming visible) replaces this behavior through
/// the normal level-change path.
pub struct SearchBehavior {
    goal: Vector3<f32>,
    steering_strategy: Box<dyn SteeringStrategy>,
    arrived: bool,
    scan_seconds: f32,
    total_seconds: f32,
}

impl SearchBehavior {
    pub fn new(goal: Vector3<f32>) -> SearchBehavior {
        SearchBehavior {
            goal,
            steering_strategy: steering::chained(vec![
                Box::new(CollisionAvoidanceSteeringStrategy::conservative()),
                // Route to the fixed last-known position; without AIPATH
                // data this returns None and the AI just scans in place
                Box::new(PathFollowSteeringStrategy::to_point(goal)),
            ]),
            arrived: false,
            scan_seconds: 0.0,
            total_seconds: 0.0,
        }
    }

    fn give_up(&self) -> bool {
        (self.arrived && self.scan_seconds >= SEARCH_SCAN_SECONDS)
            || self.total_seconds >= SEARCH_MAX_SECONDS
    }
}

impl Behavior for SearchBehavior {
    fn name(&self) -> &'static str {
        "Search"
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let dt = time.elapsed.as_secs_f32();
        self.total_seconds += dt;

        if !self.arrived {
            let (position, _) = ai_util::get_position_and_forward(world, entity_id);
            let dx = position.x - self.goal.x;
            let dz = position.z - self.goal.z;
            if (dx * dx + dz * dz).sqrt() < SEARCH_ARRIVE_DISTANCE {
                self.arrived = true;
            }
        }

        if self.arrived {
            // Stand at the last-known position; the scan animation does the
            // looking around
            self.scan_seconds += dt;
            return None;
        }

        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn animation(&self) -> Vec<MotionQueryItem> {
        if self.arrived {
            vec![
                MotionQueryItem::new("search"),
                MotionQueryItem::new("scan").optional(),
            ]
        } else {
            vec![
                MotionQueryItem::new("locourgent").optional(),
                MotionQueryItem::new("locomote"),
            ]
        }
    }

    fn is_locomotion(&self) -> bool {
        !self.arrived
    }

    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
    ) -> NextBehavior {
        if self.give_up() {
            NextBehavior::Next(Box::new(RefCell::new(WanderBehavior::new())))
        } else {
            NextBehavior::Stay
        }
    }
}
