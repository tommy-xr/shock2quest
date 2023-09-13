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
        // let player_pos = world.borrow::<UniqueView<PlayerInfo>>().unwrap().pos;
        // let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        // // let mut effects = vec![];
        // if let Ok(prop_pos) = v_current_pos.get(entity_id) {
        //     let current_yaw = ai_util::current_yaw(entity_id, world);
        //     let desired_yaw = ai_util::yaw_between_vectors(prop_pos.position, player_pos);
        //     println!(
        //         "Current Yaw: {:?} Desired yaw: {:?}",
        //         current_yaw, desired_yaw
        //     );
        //     let eff = Effect::SetRotation {
        //         entity_id,
        //         rotation: Quaternion::from_angle_y(desired_yaw),
        //     };
        //     effects.push(eff);
        // }
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();
        //let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            //let current_yaw = ai_util::current_yaw(entity_id, world);
            let _desired_yaw = ai_util::yaw_between_vectors(prop_pos.position, u_player.pos);
            return Some((
                Steering::turn_to_point(
                    vec3_to_point3(prop_pos.position),
                    vec3_to_point3(u_player.pos),
                ),
                Effect::NoEffect,
            ));
            // println!(
            //     "Current Yaw: {:?} Desired yaw: {:?}",
            //     current_yaw, desired_yaw
            // );
            // let eff = Effect::SetRotation {
            //     entity_id,
            //     rotation: Quaternion::from_angle_y(desired_yaw),
            // };
            // effects.push(eff);
        };
        None

        // let player = v_player.iter().next().unwrap();
        // let player_transform = v_transform.get(player).unwrap().0;

        // let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
        // let forward = player_transform
        //     .transform_vector(vec3(0.0, 0.0, 1.0))
        //     .normalize();
        // let target = player_transform
        //     .transform_point(point3(0.0, 0.0, 0.0))
        //     .to_vec();

        // let target = target + forward * 2.0;

        // let steering_output = Steering::turn_to_point(position, target);

        // Some((steering_output, Effect::NoEffect))
    }
}
