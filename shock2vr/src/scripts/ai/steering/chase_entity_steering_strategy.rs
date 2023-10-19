use cgmath::Deg;
use dark::properties::PropPosition;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo, physics::PhysicsWorld, scripts::Effect, time::Time, util::vec3_to_point3,
};

use super::{Steering, SteeringOutput, SteeringStrategy};

pub struct ChaseEntitySteeringStrategy(EntityId);

impl ChaseEntitySteeringStrategy {
    pub fn new(entity_id: EntityId) -> ChaseEntitySteeringStrategy {
        ChaseEntitySteeringStrategy(entity_id)
    }
}

impl SteeringStrategy for ChaseEntitySteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        world: &World,
        _physics: &PhysicsWorld,
        entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        // TODO: Check if player is visible?
        if let (Ok(prop_pos), Ok(to_pos)) =
            (v_current_pos.get(entity_id), v_current_pos.get(self.0))
        {
            return Some((
                Steering::turn_to_point(
                    vec3_to_point3(prop_pos.position),
                    vec3_to_point3(to_pos.position),
                ),
                Effect::NoEffect,
            ));
        };

        None
    }
}
