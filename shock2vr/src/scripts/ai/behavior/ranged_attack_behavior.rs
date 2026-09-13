use std::cell::RefCell;

use cgmath::Deg;
use dark::motion::MotionQueryItem;

use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{ChasePlayerSteeringStrategy, SteeringOutput, SteeringStrategy},
    },
    time::Time,
};

use super::{Behavior, ChaseBehavior, NextBehavior};

pub struct RangedAttackBehavior;

impl Behavior for RangedAttackBehavior {
    fn name(&self) -> &'static str {
        "RangedAttack"
    }

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
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        // A completed shot is not a reason to walk. Recheck the same range,
        // weapon and line-of-fire gates used when entering combat; chase
        // only when no attack is available now.
        NextBehavior::Next(
            super::attack_behavior_for_distance(world, physics, entity_id)
                .unwrap_or_else(|| Box::new(RefCell::new(ChaseBehavior::new()))),
        )
    }
}
