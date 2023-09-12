use std::{cell::RefCell, rc::Rc};

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Matrix4, Quaternion, Rotation3,
    SquareMatrix, Transform,
};
use dark::{
    motion::{MotionFlags, MotionQueryItem},
    properties::{Link, PropCreature, PropHitPoints, PropPosition},
    SCALE_FACTOR,
};

use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    creature,
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::{RuntimePropJointTransforms, RuntimePropTransform},
    time::Time,
    util,
};

use super::{
    ai::{
        self, ai_util, random_behavior,
        steering::{ChasePlayerSteeringStrategy, Steering},
        ChaseBehavior, DeadBehavior, IdleBehavior, MeleeAttackBehavior, NoopBehavior,
        WanderBehavior,
    },
    script_util::get_first_link_with_template_and_data,
    Effect, Message, MessagePayload, Script,
};

pub struct BaseMonster {
    last_hit_sensor: Option<EntityId>,
    current_behavior: Box<RefCell<dyn super::ai::Behavior>>,
    current_heading: Deg<f32>,
    is_dead: bool,
    took_damage: bool,
    animation_seq: u32,
}
impl BaseMonster {
    pub fn new() -> BaseMonster {
        BaseMonster {
            is_dead: false,
            took_damage: false,
            //current_behavior: Box::new(RefCell::new(MeleeAttackBehavior)),
            current_behavior: Box::new(RefCell::new(ChaseBehavior::new())),
            current_heading: Deg(0.0),
            animation_seq: 0,
            last_hit_sensor: None,
        }
    }

    fn apply_steering_output(
        &mut self,
        steering_output: ai::steering::SteeringOutput,
        time: &Time,
        entity_id: EntityId,
    ) -> Effect {
        let turn_velocity = self.current_behavior.borrow().turn_speed().0;
        let delta = ai_util::clamp_to_minimal_delta_angle(
            steering_output.desired_heading - self.current_heading,
        );
        // clamp_to_minimal_delta_angle(steering_output.desired_heading - self.current_heading);

        let turn_amount = if delta.0 < 0.0 {
            (-turn_velocity * time.elapsed.as_secs_f32()).max(delta.0)
        } else {
            (turn_velocity * time.elapsed.as_secs_f32()).min(delta.0)
        };

        self.current_heading = Deg(self.current_heading.0 + turn_amount);
        let rotation_effect = Effect::SetRotation {
            entity_id,
            rotation: Quaternion::from_angle_y(self.current_heading),
        };
        rotation_effect
    }

    fn try_tickle_sensor(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        let (position, forward) = ai_util::get_position_and_forward(world, entity_id);

        let down_amount = 2.0 / SCALE_FACTOR;
        let down_vector = vec3(0.0, -down_amount, 0.0);

        let distance = 8.0 / SCALE_FACTOR;

        let direction = forward + down_vector;

        let maybe_hit_result = physics.ray_cast2(
            position,
            forward + down_vector,
            distance,
            InternalCollisionGroups::ALL_COLLIDABLE,
            Some(entity_id),
            false,
        );

        let maybe_hit_sensor = if maybe_hit_result.is_some() {
            let hit_result = maybe_hit_result.unwrap();

            if hit_result.is_sensor {
                hit_result.maybe_entity_id
            } else {
                None
            }
        } else {
            None
        };

        let sensor_effect = if maybe_hit_sensor != self.last_hit_sensor {
            match maybe_hit_sensor {
                Some(sensor_id) => Effect::Send {
                    msg: Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorBeginIntersect { with: entity_id },
                    },
                },
                None => {
                    if let Some(sensor_id) = self.last_hit_sensor {
                        Effect::Send {
                            msg: Message {
                                to: sensor_id,
                                payload: MessagePayload::SensorEndIntersect { with: entity_id },
                            },
                        }
                    } else {
                        Effect::NoEffect
                    }
                }
            }
        } else {
            Effect::NoEffect
        };

        let color = if maybe_hit_sensor.is_some() {
            vec4(1.0, 1.0, 0.0, 1.0)
        } else {
            vec4(0.0, 1.0, 1.0, 1.0)
        };

        self.last_hit_sensor = maybe_hit_sensor;

        let debug_effect = Effect::DrawDebugLines {
            lines: vec![(
                position,
                position + ((forward + down_vector) * distance),
                color,
            )],
        };

        Effect::combine(vec![sensor_effect, debug_effect])
    }
}
impl Script for BaseMonster {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.current_heading = ai_util::current_yaw(entity_id, world);
        Effect::QueueAnimationBySchema {
            entity_id,
            motion_query_items: self.current_behavior.borrow().animation(),
            selection_strategy: dark::motion::MotionQuerySelectionStrategy::Sequential(
                self.animation_seq,
            ), //     MotionQueryItem::new("rangedcombat".to_owned())),
               //     // "rangedcombat".to_owned(),
               //     // "attack".to_owned(),
               //     //"direction".to_owned(),
               // ],
        }

        // Effect::QueueAnimationBySchema {
        //     entity_id,
        //     motion_query_items: self.current_behavior.animation(),
        // }
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        // Temporary steering behavior
        let (steering_output, steering_effects) = self
            .current_behavior
            .borrow_mut()
            .steer(self.current_heading, world, physics, entity_id, time)
            .unwrap_or((
                Steering::from_current(self.current_heading),
                Effect::NoEffect,
            ));

        let rotation_effect = self.apply_steering_output(steering_output, time, entity_id);

        let debug_effect = draw_debug_facing_line(world, entity_id);

        let sensor_effect = self.try_tickle_sensor(world, physics, entity_id);

        Effect::combine(vec![
            steering_effects,
            rotation_effect,
            debug_effect,
            sensor_effect,
        ])
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Damage { amount } => {
                self.took_damage = true;
                Effect::AdjustHitPoints {
                    entity_id,
                    delta: -1 * amount.round() as i32,
                }
            }
            MessagePayload::AnimationCompleted => {
                if self.is_dead {
                    Effect::NoEffect
                } else if is_killed(entity_id, world) {
                    self.current_behavior = Box::new(RefCell::new(DeadBehavior {}));
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: vec![MotionQueryItem::new("crumple")],
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
                    }
                } else if self.took_damage {
                    self.took_damage = false;
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: vec![MotionQueryItem::new("receivewound")],
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
                    }
                } else {
                    let next_behavior = {
                        self.current_behavior
                            .borrow()
                            .next_behavior(world, physics, entity_id)
                    };

                    match next_behavior {
                        ai::NextBehavior::NoOpinion => (),
                        ai::NextBehavior::Stay => (),
                        ai::NextBehavior::Next(behavior) => {
                            self.current_behavior = behavior;
                        }
                    };
                    //self.current_behavior = Rc::new(IdleBehavior);
                    self.animation_seq = self.animation_seq + 1;
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: self.current_behavior.borrow().animation(),
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Sequential(
                            self.animation_seq,
                        ),
                        //tag: "idlegesture".to_owned(),
                        // motion_query_items: vec![
                        //     MotionQueryItem::new("search"),
                        //     MotionQueryItem::new("scan").optional(),
                        // -- Walk around items
                        // MotionQueryItem::new("locomote").optional(),
                        // MotionQueryItem::new("search").optional(),
                        // --

                        // Die
                        // MotionQueryItem::new("crumple").optional(),
                        // MotionQueryItem::new("grunt").optional(),
                        // MotionQueryItem::new("pipe").optional(),
                        // --

                        // --- Melee attack items
                        // MotionQueryItem::new("meleecombat").optional(),
                        // MotionQueryItem::new("attack").optional(),
                        // MotionQueryItem::new("direction").optional(),
                        // ---

                        // --- Ranged combat attack items
                        // MotionQueryItem::new("rangedcombat").optional(),
                        // MotionQueryItem::new("attack").optional(),
                        // MotionQueryItem::new("direction").optional(),
                        // ---

                        //MotionQueryItem::new("search").optional(),
                        // MotionQueryItem::new("locourgent").optional(),
                        //MotionQueryItem::new("attack"),
                        // MotionQueryItem::new("stand"),
                        //MotionQueryItem::new("direction").optional(),
                        //],
                    }
                }
            }
            MessagePayload::AnimationFlagTriggered { motion_flags } => {
                if motion_flags.contains(MotionFlags::FIRE) {
                    fire_ranged_projectile(world, entity_id)
                // } else if motion_flags.contains(MotionFlags::END) {
                //     Effect::QueueAnimationBySchema {
                //         entity_id,
                //         motion_query_items: vec![MotionQueryItem::new("rangedcombat")],
                //         //     MotionQueryItem::new("rangedcombat".to_owned())),
                //         //     // "rangedcombat".to_owned(),
                //         //     // "attack".to_owned(),
                //         //     //"direction".to_owned(),
                //         // ],
                //     }
                } else if motion_flags.contains(MotionFlags::UNK7 /* die? */) {
                    self.is_dead = true;
                    Effect::NoEffect
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

fn draw_debug_facing_line(world: &World, entity_id: EntityId) -> Effect {
    let xform = world
        .borrow::<View<RuntimePropTransform>>()
        .unwrap()
        .get(entity_id)
        .unwrap()
        .0;

    let position = util::get_position_from_matrix(&xform);
    let forward = xform.transform_vector(vec3(0.0, 0.0, 1.0)).normalize();
    let debug_effect = Effect::DrawDebugLines {
        lines: vec![(
            position + vec3(0.0, 0.5, 0.0),
            position + forward + vec3(0.0, 0.5, 0.0),
            vec4(1.0, 0.0, 0.0, 1.0),
        )],
    };
    debug_effect
}

fn fire_ranged_projectile(world: &World, entity_id: EntityId) -> Effect {
    let maybe_projectile =
        get_first_link_with_template_and_data(world, entity_id, |link| match link {
            Link::AIProjectile(data) => Some(*data),
            _ => None,
        });

    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_joint_transforms = world.borrow::<View<RuntimePropJointTransforms>>().unwrap();

    let v_creature = world.borrow::<View<PropCreature>>().unwrap();
    if let Some((projectile_id, options)) = maybe_projectile {
        let root_transform = v_transform.get(entity_id).unwrap();
        let forward = vec3(0.0, 0.0, -1.0);
        let _up = vec3(0.0, 1.0, 0.0);

        let creature_type = v_creature.get(entity_id).unwrap();
        let joint_index = creature::get_creature_definition(creature_type.0)
            .and_then(|def| def.get_mapped_joint(options.joint))
            .unwrap_or(0);
        let joint_transform = v_joint_transforms
            .get(entity_id)
            .map(|transform| transform.0.get(joint_index as usize))
            .ok()
            .flatten()
            .copied()
            .unwrap_or(Matrix4::identity());

        let transform = root_transform.0 * joint_transform;

        //let orientation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0));
        let _position = joint_transform.transform_point(point3(0.0, 0.0, 0.0));

        let rotation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0));
        // TODO: This rotation is needed for some monsters? Like the droids?
        let _rot_matrix: Matrix4<f32> = Matrix4::from(rotation);

        // panic!("creating entity: {:?}", projectile_id);
        Effect::CreateEntity {
            template_id: projectile_id,
            position: forward * 0.75,
            // position: vec3(13.11, 0.382, 16.601),
            // orientation: rotation,
            orientation: Quaternion {
                v: vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            velocity: vec3(0.0, 0.0, 0.0),
            // root_transform: transform * rot_matrix,
            root_transform: transform,
        }
    } else {
        Effect::NoEffect
    }
}

fn is_killed(entity_id: EntityId, world: &World) -> bool {
    let v_prop_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();

    let maybe_prop_hit_points = v_prop_hit_points.get(entity_id);
    if maybe_prop_hit_points.is_err() {
        return false;
    }

    maybe_prop_hit_points.unwrap().hit_points <= 0
}
