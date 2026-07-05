use std::cell::RefCell;

use cgmath::*;
use dark::motion::MotionQueryItem;
use rand::Rng;
use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect, MessagePayload,
        ai::steering::{Steering, SteeringOutput},
    },
    time::Time,
};

use super::WanderBehavior;

pub enum NextBehavior {
    NoOpinion,
    Next(Box<RefCell<dyn Behavior>>),
    Stay,
}

/// Whether a behavior is a data-authored scripted sequence, and if so whether
/// it is still performing. Alertness changes must not preempt a running
/// sequence (the original engine gates this via the response's priority
/// field; we currently treat every sequence as protected).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptedState {
    NotScripted,
    Running,
    Finished,
}

pub trait Behavior {
    /// Short stable name for debug introspection (e.g. "Chase", "Wander")
    fn name(&self) -> &'static str;

    fn scripted_state(&self) -> ScriptedState {
        ScriptedState::NotScripted
    }

    /// For scripted sequences: the signal that started them, if any.
    fn origin_signal(&self) -> Option<&str> {
        None
    }

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

    fn is_locomotion(&self) -> bool {
        false
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
