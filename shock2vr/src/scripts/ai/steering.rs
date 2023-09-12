use std::{cell::RefCell, rc::Rc};

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3,
};
use dark::{motion::MotionQueryItem, properties::PropPosition, SCALE_FACTOR};
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::RuntimePropTransform,
    scripts::ai::ai_util::random_binomial,
    time::Time,
    util::{
        get_position_from_matrix, get_position_from_transform, get_rotation_from_forward_vector,
        get_rotation_from_matrix, get_rotation_from_transform, vec3_to_point3,
    },
};

use super::{
    ai_util::{self, is_entity_door},
    Effect, MessagePayload,
};

pub struct SteeringOutput {
    pub desired_heading: Deg<f32>,
}

impl Default for SteeringOutput {
    fn default() -> Self {
        SteeringOutput {
            desired_heading: Deg(0.0),
        }
    }
}
pub struct Steering;

impl Steering {
    pub fn from_current(heading: Deg<f32>) -> SteeringOutput {
        SteeringOutput {
            desired_heading: heading,
        }
    }

    pub fn turn_to_point(position: Point3<f32>, target: Point3<f32>) -> SteeringOutput {
        let yaw = ai_util::yaw_between_vectors(position.to_vec(), target.to_vec());
        SteeringOutput {
            desired_heading: yaw,
        }
    }
}

pub trait SteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        None
    }
}

pub fn chained(strategies: Vec<Box<dyn SteeringStrategy>>) -> Box<dyn SteeringStrategy> {
    Box::new(ChainedSteeringStrategy {
        strategies: strategies,
    })
}

pub struct ChainedSteeringStrategy {
    strategies: Vec<Box<dyn SteeringStrategy>>,
}

impl SteeringStrategy for ChainedSteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        for strategy in &mut self.strategies {
            if let Some((steering_output, effect)) =
                strategy.steer(_current_heading, _world, _physics, _entity_id, _time)
            {
                return Some((steering_output, effect));
            }
        }

        None
    }
}

pub struct WanderSteeringStrategy {
    maybe_current_heading: Option<Deg<f32>>,
}

impl WanderSteeringStrategy {
    pub fn new() -> WanderSteeringStrategy {
        WanderSteeringStrategy {
            maybe_current_heading: None,
        }
    }

    pub fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        if let Some(current_heading) = self.maybe_current_heading {
            self.maybe_current_heading = Some(Deg(
                current_heading.0 + 100.0 * random_binomial() * time.elapsed.as_secs_f32()
            ))
        } else {
            self.maybe_current_heading = Some(current_heading);
        };
        println!("steering output: {:?}", self.maybe_current_heading);
        Some((
            SteeringOutput {
                desired_heading: self.maybe_current_heading.unwrap(),
            },
            Effect::NoEffect,
        ))
    }
}

pub struct CollisionAvoidanceSteeringStrategy {
    extra_whisker_distance: f32,
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
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        // TODO
        let height = 1.0 / SCALE_FACTOR;
        let whisker_angle = Deg(30.0);
        let target_offset = 8.0 / SCALE_FACTOR;

        let position = point3(0.0, -height, 0.0)
            + get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));

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
        let mut dist = f32::MAX;
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

pub struct ChasePlayerSteeringStrategy;

impl SteeringStrategy for ChasePlayerSteeringStrategy {
    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
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
            let desired_yaw = ai_util::yaw_between_vectors(prop_pos.position, u_player.pos);
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
