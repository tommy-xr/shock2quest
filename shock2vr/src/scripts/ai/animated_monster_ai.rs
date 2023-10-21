use std::cell::RefCell;

use cgmath::{vec3, vec4, Deg, Quaternion, Rotation3};
use dark::{
    motion::{MotionFlags, MotionQueryItem},
    properties::PropAISignalResponse,
    SCALE_FACTOR,
};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    time::Time,
};

use super::{
    ai_util::*,
    behavior::*,
    steering::{Steering, SteeringOutput},
    Effect, Message, MessagePayload, Script,
};
pub struct AnimatedMonsterAI {
    last_hit_sensor: Option<EntityId>,
    current_behavior: Box<RefCell<dyn Behavior>>,
    current_heading: Deg<f32>,
    is_dead: bool,
    took_damage: bool,
    animation_seq: u32,
}

impl AnimatedMonsterAI {
    pub fn idle() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            took_damage: false,
            //current_behavior: Box::new(RefCell::new(MeleeAttackBehavior)),
            //current_behavior: Box::new(RefCell::new(ChaseBehavior::new())),
            current_behavior: Box::new(RefCell::new(IdleBehavior)),
            current_heading: Deg(0.0),
            animation_seq: 0,
            last_hit_sensor: None,
        }
    }
    pub fn new() -> AnimatedMonsterAI {
        AnimatedMonsterAI {
            is_dead: false,
            took_damage: false,
            //current_behavior: Box::new(RefCell::new(MeleeAttackBehavior)),
            //current_behavior: Box::new(RefCell::new(ChaseBehavior::new())),
            current_behavior: Box::new(RefCell::new(RangedAttackBehavior)),
            current_heading: Deg(0.0),
            animation_seq: 0,
            last_hit_sensor: None,
        }
    }

    fn apply_steering_output(
        &mut self,
        steering_output: SteeringOutput,
        time: &Time,
        entity_id: EntityId,
    ) -> Effect {
        let turn_velocity = self.current_behavior.borrow().turn_speed().0;
        let delta =
            clamp_to_minimal_delta_angle(steering_output.desired_heading - self.current_heading);

        let turn_amount = if delta.0 < 0.0 {
            (-turn_velocity * time.elapsed.as_secs_f32()).max(delta.0)
        } else {
            (turn_velocity * time.elapsed.as_secs_f32()).min(delta.0)
        };

        self.current_heading = Deg(self.current_heading.0 + turn_amount);

        Effect::SetRotation {
            entity_id,
            rotation: Quaternion::from_angle_y(self.current_heading),
        }
    }

    fn try_tickle_sensor(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Effect {
        let (position, forward) = get_position_and_forward(world, entity_id);

        let down_amount = 2.0 / SCALE_FACTOR;
        let down_vector = vec3(0.0, -down_amount, 0.0);

        let distance = 8.0 / SCALE_FACTOR;

        let _direction = forward + down_vector;

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

impl Script for AnimatedMonsterAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.current_heading = current_yaw(entity_id, world);
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
        {
            self.current_behavior
                .borrow_mut()
                .handle_message(entity_id, world, physics, msg);
        }
        match msg {
            MessagePayload::Damage { amount } => {
                self.took_damage = true;
                Effect::AdjustHitPoints {
                    entity_id,
                    delta: -(amount.round() as i32),
                }
            }
            MessagePayload::Signal { name } => {
                // Do we have a response to this signal?

                let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

                if let Ok(prop_sig_resp) = v_prop_sig_resp.get(entity_id) {
                    // Immediately switch to Scripted sequence Behavior
                    self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                        world,
                        prop_sig_resp.actions.clone(),
                    )));
                    self.animation_seq += 1;
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: self.current_behavior.borrow().animation(),
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Sequential(
                            self.animation_seq,
                        ),
                    }
                } else {
                    Effect::NoEffect
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
                            .borrow_mut()
                            .next_behavior(world, physics, entity_id)
                    };

                    match next_behavior {
                        NextBehavior::NoOpinion => (),
                        NextBehavior::Stay => (),
                        NextBehavior::Next(behavior) => {
                            self.current_behavior = behavior;
                        }
                    };
                    //self.current_behavior = Rc::new(IdleBehavior);
                    self.animation_seq += 1;
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
