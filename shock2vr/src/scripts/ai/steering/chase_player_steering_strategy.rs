use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3,
};
use dark::{properties::PropPosition, SCALE_FACTOR};

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{
        ai::ai_util::{self, random_binomial},
        Effect,
    },
    time::Time,
    util::{get_position_from_transform, get_rotation_from_transform, vec3_to_point3},
};

use super::{Steering, SteeringOutput, SteeringStrategy};

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
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        // TODO: Check if player is visible?
        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            return Some((
                Steering::turn_to_point(
                    vec3_to_point3(prop_pos.position),
                    vec3_to_point3(u_player.pos),
                ),
                Effect::NoEffect,
            ));
        };

        None
    }
}
