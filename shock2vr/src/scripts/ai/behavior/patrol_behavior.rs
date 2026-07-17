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

use std::cell::RefCell;

use super::{Behavior, IdleBehavior, NextBehavior};

/// Close enough to a patrol point to advance to the next one
const PATROL_ARRIVE_DISTANCE: f32 = 4.0 / SCALE_FACTOR;
/// ...and within this height difference (patrol markers sit near the floor).
/// Standing under a marker on a walkway above is not arrival.
const PATROL_ARRIVE_HEIGHT: f32 = 6.0 / SCALE_FACTOR;

/// Walk an authored patrol route: head to the current patrol point, and on
/// arrival advance to the next one along the `AIPatrol` link chain. Routes are
/// typically closed loops, so this repeats indefinitely. Alertness changes (the
/// player becoming visible, a noise) replace this behavior through the normal
/// level-change path.
pub struct PatrolBehavior {
    /// The patrol-point object we are currently heading to
    target_point: EntityId,
    /// Its world position (the steering goal)
    goal: Vector3<f32>,
    steering_strategy: Box<dyn SteeringStrategy>,
    /// Set when the route dead-ends (a chain that isn't a loop): the AI has
    /// reached the final point and there is nowhere further to go, so it stops
    /// patrolling and hands back to idle.
    finished: bool,
}

impl PatrolBehavior {
    pub fn new(target_point: EntityId, goal: Vector3<f32>) -> PatrolBehavior {
        PatrolBehavior {
            target_point,
            goal,
            steering_strategy: Self::steering_to(goal),
            finished: false,
        }
    }

    fn steering_to(goal: Vector3<f32>) -> Box<dyn SteeringStrategy> {
        steering::chained(vec![
            // Path steering leads; whisker avoidance only covers the no-route
            // case (see chase_behavior for the deadlock this prevents)
            Box::new(PathFollowSteeringStrategy::to_point(goal)),
            Box::new(CollisionAvoidanceSteeringStrategy::conservative()),
        ])
    }

    fn arrived(&self, world: &World, entity_id: EntityId) -> bool {
        let (position, _) = ai_util::get_position_and_forward(world, entity_id);
        let dx = position.x - self.goal.x;
        let dz = position.z - self.goal.z;
        (dx * dx + dz * dz).sqrt() < PATROL_ARRIVE_DISTANCE
            && (position.y - self.goal.y).abs() < PATROL_ARRIVE_HEIGHT
    }
}

impl Behavior for PatrolBehavior {
    fn name(&self) -> &'static str {
        "Patrol"
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        if !self.finished && self.arrived(world, entity_id) {
            // Advance to the next point on the route; a closed loop repeats.
            // A dead-end (no next link) ends the patrol - stop and hand back to
            // idle via next_behavior, rather than walking in place forever.
            match ai_util::next_patrol_point(world, self.target_point) {
                Some((next, goal)) => {
                    self.target_point = next;
                    self.goal = goal;
                    self.steering_strategy = Self::steering_to(goal);
                }
                None => self.finished = true,
            }
        }

        if self.finished {
            return None;
        }

        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
    ) -> NextBehavior {
        if self.finished {
            NextBehavior::Next(Box::new(RefCell::new(IdleBehavior)))
        } else {
            NextBehavior::Stay
        }
    }

    fn animation(&self) -> Vec<MotionQueryItem> {
        if self.finished {
            return vec![MotionQueryItem::new("idlegesture").optional()];
        }
        vec![
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::new("locomote"),
        ]
    }

    fn is_locomotion(&self) -> bool {
        !self.finished
    }
}
