use cgmath::{Deg, InnerSpace};
use dark::SCALE_FACTOR;
use dark::properties::PropPosition;

use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld, scripts::Effect, scripts::ai::ai_util, time::Time, util::vec3_to_point3,
};

use super::{Steering, SteeringOutput, SteeringStrategy};

/// Standing on the target position, there is nowhere left to steer (2 Dark
/// feet) - without this an AI that reached a stale last-known position spins
/// on heading noise
const CHASE_ARRIVE_DISTANCE: f32 = 2.0 / SCALE_FACTOR;

pub struct ChasePlayerSteeringStrategy;

impl SteeringStrategy for ChasePlayerSteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        world: &World,
        _physics: &PhysicsWorld,
        entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        // Pursue what this AI KNOWS (its last-known target position, frozen
        // when sight breaks) rather than the player's true location
        let target = ai_util::chase_target(world, entity_id)?;
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            if (target - prop_pos.position).magnitude() < CHASE_ARRIVE_DISTANCE {
                return None;
            }
            return Some((
                Steering::turn_to_point(vec3_to_point3(prop_pos.position), vec3_to_point3(target)),
                Effect::NoEffect,
            ));
        };

        None
    }
}
