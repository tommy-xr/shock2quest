use std::cell::RefCell;

use cgmath::*;
use dark::motion::MotionQueryItem;
use rand::Rng;
use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        ai::steering::{Steering, SteeringOutput},
        Effect, MessagePayload,
    },
    time::Time,
};

use super::WanderBehavior;

pub enum NextBehavior {
    NoOpinion,
    Next(Box<RefCell<dyn Behavior>>),
    Stay,
}

pub trait Behavior {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![]
    }

    ///
    /// turn_speed
    ///
    /// Turn speed of the character in degrees / s
    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }

    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
    ) -> NextBehavior {
        NextBehavior::NoOpinion
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}

#[allow(dead_code)]
pub fn random_behavior() -> Box<RefCell<dyn Behavior>> {
    let mut potential_behaviors: Vec<Box<RefCell<dyn Behavior>>> = vec![
        // Rc::new(MeleeAttackBehavior),
        // Rc::new(SearchBehavior),
        Box::new(RefCell::new(WanderBehavior::new())),
        //Rc::new(IdleBehavior),
        // Rc::new(RangedAttackBehavior),
        //Rc::new(ChaseBehavior),
        //Rc::new(DieBehavior),
    ];
    let mut rng = rand::thread_rng();
    let idx = rng.gen_range(0..potential_behaviors.len());
    potential_behaviors.remove(idx)
}
