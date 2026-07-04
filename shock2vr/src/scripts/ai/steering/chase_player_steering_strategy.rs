use cgmath::Deg;
use dark::SCALE_FACTOR;
use dark::properties::PropPosition;

use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld, scripts::Effect, scripts::ai::ai_util, time::Time, util::vec3_to_point3,
};

use super::{Steering, SteeringOutput, SteeringStrategy};

/// On the target position there is nowhere left to steer (2 Dark feet XZ,
/// 6 ft height tolerance - the recorded position sits eye-height above the
/// floor). This stops the heading from spinning on noise; locomotion root
/// motion may still pace nearby until Search takes over on the next decay.
const CHASE_ARRIVE_DISTANCE: f32 = 2.0 / SCALE_FACTOR;
const CHASE_ARRIVE_HEIGHT: f32 = 6.0 / SCALE_FACTOR;

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
            let dx = target.x - prop_pos.position.x;
            let dz = target.z - prop_pos.position.z;
            let dy = (target.y - prop_pos.position.y).abs();
            if (dx * dx + dz * dz).sqrt() < CHASE_ARRIVE_DISTANCE && dy < CHASE_ARRIVE_HEIGHT {
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
