use std::cell::RefCell;

use cgmath::{Deg, InnerSpace};
use dark::{SCALE_FACTOR, motion::MotionQueryItem, properties::PropPosition};
use rand::Rng;
use shipyard::*;

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{
            self, ChasePlayerSteeringStrategy, CollisionAvoidanceSteeringStrategy, SteeringOutput,
            SteeringStrategy,
        },
    },
    time::Time,
};

use super::{Behavior, MeleeAttackBehavior, NextBehavior, RangedAttackBehavior};

pub struct ChaseBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl ChaseBehavior {
    pub fn new() -> ChaseBehavior {
        ChaseBehavior {
            steering_strategy: steering::chained(vec![
                Box::new(
                    CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
                ),
                Box::new(ChasePlayerSteeringStrategy),
            ]),
        }
    }
}

impl Behavior for ChaseBehavior {
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
        _physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        let _rand = rand::thread_rng().gen_range(0..100);
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();
        //let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();

        let melee_attack_distance = 8.0 / SCALE_FACTOR;
        let ranged_max_attack_distance = 40.0 / SCALE_FACTOR;
        let ranged_min_attack_distance = 15.0 / SCALE_FACTOR;

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            let distance = (prop_pos.position - u_player.pos).magnitude();

            if distance > ranged_min_attack_distance && distance < ranged_max_attack_distance {
                return NextBehavior::Next(Box::new(RefCell::new(RangedAttackBehavior)));
            }
            if distance < melee_attack_distance {
                return NextBehavior::Next(Box::new(RefCell::new(MeleeAttackBehavior)));
            }
        }

        NextBehavior::Stay
    }
}
