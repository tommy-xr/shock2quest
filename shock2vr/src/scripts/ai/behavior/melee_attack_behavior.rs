use std::cell::RefCell;

use cgmath::Deg;
use dark::{SCALE_FACTOR, motion::MotionQueryItem};

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

pub struct MeleeAttackBehavior;

impl Behavior for MeleeAttackBehavior {
    fn combat_mode(&self) -> Option<super::CombatMode> {
        Some(super::CombatMode::Melee)
    }

    fn name(&self) -> &'static str {
        "MeleeAttack"
    }

    fn animation(self: &MeleeAttackBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("meleecombat"),
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
        _physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        // Gate on the KNOWN target distance (consistent with where the
        // attack faces), not the player's true position - a player sneaking
        // behind an attacking AI must not pin it swinging at empty space
        let melee_attack_distance = 8.0 / SCALE_FACTOR;
        if let Some(distance) = crate::scripts::ai::ai_util::chase_target_distance(world, entity_id)
        {
            if distance < melee_attack_distance {
                return NextBehavior::Stay;
            }
        }
        NextBehavior::Next(Box::new(RefCell::new(ChaseBehavior::new())))
    }
}
