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

    fn combat_mode(&self) -> Option<super::CombatMode> {
        None
    }
    fn is_combat_frustration(&self) -> bool {
        false
    }

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

    /// Motion schema queries to try in priority order for this behavior.
    fn animation_queries(&self) -> Vec<Vec<MotionQueryItem>> {
        vec![self.animation()]
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

    /// Stop root-motion travel while retaining the current pose/heading.
    fn holds_position(&self) -> bool {
        false
    }

    fn is_locomotion(&self) -> bool {
        false
    }

    /// Whether an alertness level change may replace this behavior. False for
    /// a behavior counting down a commitment it must not restart (see
    /// `SelfDestructBehavior`).
    fn preempted_by_alertness(&self) -> bool {
        true
    }

    /// Live patrol destination, exposed to focused behavior tests without
    /// downcasting trait objects.
    #[cfg(test)]
    fn patrol_target(&self) -> Option<EntityId> {
        None
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
