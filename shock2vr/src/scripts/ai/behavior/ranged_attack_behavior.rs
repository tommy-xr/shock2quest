use std::cell::RefCell;

use cgmath::{Deg, InnerSpace};
use dark::{motion::MotionQueryItem, properties::PropPosition, SCALE_FACTOR};
use rand::Rng;
use shipyard::*;

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    scripts::{
        ai::steering::{
            self, ChasePlayerSteeringStrategy, CollisionAvoidanceSteeringStrategy, Steering,
            SteeringOutput, SteeringStrategy,
        },
        Effect,
    },
    time::Time,
};

use super::{Behavior, ChaseBehavior, NextBehavior};

pub struct RangedAttackBehavior;

impl Behavior for RangedAttackBehavior {
    fn animation(self: &RangedAttackBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("rangedcombat").optional(),
            MotionQueryItem::new("attack").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        ChasePlayerSteeringStrategy.steer(current_heading, world, physics, entity_id, time)
    }

    fn next_behavior(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        NextBehavior::Next(Box::new(RefCell::new(ChaseBehavior::new())))
    }
}
