use cgmath::Deg;
use dark::motion::MotionQueryItem;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        ai::steering::{Steering, SteeringOutput},
        Effect,
    },
    time::Time,
};

use super::Behavior;

pub struct DeadBehavior {}

impl Behavior for DeadBehavior {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(0.0)
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

    fn animation(self: &DeadBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("crumple").optional()]
    }
}
