use std::cell::RefCell;

use cgmath::{Deg, InnerSpace};
use dark::{SCALE_FACTOR, motion::MotionQueryItem, properties::PropPosition};

use shipyard::*;

use crate::{
    mission::PlayerInfo,
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
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        let melee_attack_distance = 8.0 / SCALE_FACTOR;

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            let distance = (prop_pos.position - u_player.pos).magnitude();

            if distance < melee_attack_distance {
                return NextBehavior::Stay;
            }
        }
        NextBehavior::Next(Box::new(RefCell::new(ChaseBehavior::new())))
    }
}
