use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, vec3, vec4};
use dark::SCALE_FACTOR;

use shipyard::{EntityId, World};

use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{
        Effect,
        ai::ai_util::{self},
    },
    time::Time,
    util::{get_position_from_transform, get_rotation_from_transform},
};

use super::{Steering, SteeringOutput, SteeringStrategy};

pub struct CollisionAvoidanceSteeringStrategy {
    #[allow(dead_code)]
    extra_whisker_distance: f32,
    #[allow(dead_code)]
    main_whisker_distance: f32,
    last_mitigation_direction: Deg<f32>,
}

impl CollisionAvoidanceSteeringStrategy {
    pub fn conservative() -> CollisionAvoidanceSteeringStrategy {
        CollisionAvoidanceSteeringStrategy {
            last_mitigation_direction: Deg(30.0),
            extra_whisker_distance: 3.0 / SCALE_FACTOR,
            main_whisker_distance: 3.0 / SCALE_FACTOR,
        }
    }

    pub fn comprehensive() -> CollisionAvoidanceSteeringStrategy {
        CollisionAvoidanceSteeringStrategy {
            last_mitigation_direction: Deg(30.0),
            extra_whisker_distance: 8.0 / SCALE_FACTOR,
            main_whisker_distance: 6.0 / SCALE_FACTOR,
        }
    }
}

impl SteeringStrategy for CollisionAvoidanceSteeringStrategy {
    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        // TODO
        let height = 1.0 / SCALE_FACTOR;
        let whisker_angle = Deg(30.0);
        let target_offset = 8.0 / SCALE_FACTOR;

        let position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0))
            + vec3(0.0, -height, 0.0);

        let mut debug_lines = vec![];

        let left_whisker_rotation = Quaternion::from_angle_y(-whisker_angle);
        let right_whisker_rotation = Quaternion::from_angle_y(whisker_angle);
        let rotation = get_rotation_from_transform(world, entity_id);

        let extra_whisker_distance = 3.0 / SCALE_FACTOR;
        let main_whisker_distance = 3.0 / SCALE_FACTOR;

        let whiskers_to_check = vec![
            (
                rotation * left_whisker_rotation,
                extra_whisker_distance,
                Some(Deg(30.0)),
            ),
            (
                rotation * right_whisker_rotation,
                extra_whisker_distance,
                Some(Deg(-30.0)),
            ),
            (rotation, main_whisker_distance, Some(Deg(180.0))),
            //(rotation, main_whisker_distance, None),
        ];

        let mut maybe_steering_output = None;
        let _dist = f32::MAX;
        for (rotation, distance, mitigation) in whiskers_to_check {
            let forward = rotation.rotate_vector(vec3(0.0, 0.0, 1.0)).normalize();
            let end_point = position + forward * distance;

            let main_whisker_hit_result = physics.ray_cast2(
                position,
                forward,
                distance,
                InternalCollisionGroups::ALL_COLLIDABLE,
                Some(entity_id),
                true,
            );

            if let Some(main_whisker_hit_result) = main_whisker_hit_result {
                debug_lines.push((
                    position,
                    main_whisker_hit_result.hit_point,
                    vec4(0.0, 0.0, 1.0, 1.0),
                ));
                let target = main_whisker_hit_result.hit_point
                    + main_whisker_hit_result.hit_normal * target_offset;

                debug_lines.push((
                    main_whisker_hit_result.hit_point,
                    target,
                    vec4(1.0, 1.0, 0.0, 1.0),
                ));

                // If the normal is valid, we can use it to calculate a new heading
                // Ignore the normal if it's pointing up or down

                let is_normal_mostly_vertical = main_whisker_hit_result.hit_normal.y.abs() > 0.5;
                let entity_is_door = main_whisker_hit_result
                    .maybe_entity_id
                    .map(|m| ai_util::is_entity_door(world, m))
                    .unwrap_or(false);

                // let distance_to_hit = (main_whisker_hit_result.hit_point - position).magnitude();
                // if distance_to_hit < dist {
                // dist = distance_to_hit;

                if !is_normal_mostly_vertical && !entity_is_door {
                    if mitigation.is_none() {
                        maybe_steering_output = Some(Steering::from_current(
                            current_heading + self.last_mitigation_direction,
                        ));
                    } else {
                        self.last_mitigation_direction = mitigation.unwrap();
                        maybe_steering_output = Some(Steering::from_current(
                            current_heading + mitigation.unwrap(),
                        ));
                    }
                }
                // }
                // }
            } else {
                debug_lines.push((position, end_point, vec4(0.0, 1.0, 0.0, 1.0)));
            };
        }

        if maybe_steering_output.is_some() {
            Some((
                maybe_steering_output.unwrap(),
                Effect::DrawDebugLines { lines: debug_lines },
            ))
        } else {
            None
        }
    }
}
