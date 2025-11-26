use std::{cell::RefCell, collections::HashSet};

use cgmath::{Deg, MetricSpace, Quaternion, Rotation3, vec3, vec4};
use dark::{
    SCALE_FACTOR,
    motion::{MotionFlags, MotionQueryItem},
    properties::{Link, PropAISignalResponse, PropPosition},
};
use rand;
use shipyard::{EntityId, Get, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::script_util,
    time::Time,
};

use super::{
    Effect, Message, MessagePayload, Script,
    ai_util::*,
    behavior::*,
    steering::{Steering, SteeringOutput},
};
pub struct AnimatedMonsterAI {
    last_hit_sensor: Option<EntityId>,
    current_behavior: Box<RefCell<dyn Behavior>>,
    current_heading: Deg<f32>,
    is_dead: bool,
    took_damage: bool,
    animation_seq: u32,
    locomotion_seq: u32,

    played_ai_watch_obj: HashSet<EntityId>,
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
            locomotion_seq: 0,
            last_hit_sensor: None,

            played_ai_watch_obj: HashSet::new(),
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
            locomotion_seq: 0,
            last_hit_sensor: None,
            played_ai_watch_obj: HashSet::new(),
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

    fn next_selection(
        &mut self,
        is_locomotion: bool,
    ) -> dark::motion::MotionQuerySelectionStrategy {
        if is_locomotion {
            let seq = self.locomotion_seq;
            self.locomotion_seq = self.locomotion_seq.wrapping_add(1);
            dark::motion::MotionQuerySelectionStrategy::Sequential(seq)
        } else {
            let seq = self.animation_seq;
            self.animation_seq = self.animation_seq.wrapping_add(1);
            dark::motion::MotionQuerySelectionStrategy::Sequential(seq)
        }
    }
}

impl Script for AnimatedMonsterAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.current_heading = current_yaw(entity_id, world);
        let is_locomotion = self.current_behavior.borrow().is_locomotion();
        let selection_strategy = self.next_selection(is_locomotion);
        Effect::QueueAnimationBySchema {
            entity_id,
            motion_query_items: self.current_behavior.borrow().animation(),
            selection_strategy, //     MotionQueryItem::new("rangedcombat".to_owned())),
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
        // Check our AIWatchObj status
        let ai_signal_resp =
            script_util::get_all_links_with_data(world, entity_id, |link| match link {
                Link::AIWatchObj(data) => Some(data.clone()),
                _ => None,
            });

        for (entity_id, watch_options) in ai_signal_resp {
            if self.played_ai_watch_obj.contains(&entity_id) {
                continue;
            }

            if player_is_within_watch_obj(world, entity_id, watch_options.radius) {
                // Immediately switch to Scripted sequence Behavior
                self.played_ai_watch_obj.insert(entity_id);
                self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                    world,
                    watch_options.scripted_actions.clone(),
                )));
                let is_locomotion = self.current_behavior.borrow().is_locomotion();
                let selection_strategy = self.next_selection(is_locomotion);
                return Effect::QueueAnimationBySchema {
                    entity_id,
                    motion_query_items: self.current_behavior.borrow().animation(),
                    selection_strategy,
                };
            }
        }

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
                // TODO: Let behavior handle this?
                //self.took_damage = true;
                Effect::AdjustHitPoints {
                    entity_id,
                    delta: -(amount.round() as i32),
                }
            }
            MessagePayload::TurnOn { from: _ } => {
                let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

                if let Ok(prop_sig_resp) = v_prop_sig_resp.get(entity_id) {
                    // Immediately switch to Scripted sequence Behavior
                    self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                        world,
                        prop_sig_resp.actions.clone(),
                    )));
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: self.current_behavior.borrow().animation(),
                        selection_strategy,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Signal { name: _ } => {
                // Do we have a response to this signal?

                let v_prop_sig_resp = world.borrow::<View<PropAISignalResponse>>().unwrap();

                if let Ok(prop_sig_resp) = v_prop_sig_resp.get(entity_id) {
                    // Immediately switch to Scripted sequence Behavior
                    self.current_behavior = Box::new(RefCell::new(ScriptedSequenceBehavior::new(
                        world,
                        prop_sig_resp.actions.clone(),
                    )));
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: self.current_behavior.borrow().animation(),
                        selection_strategy,
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

                    // Play death sound effect immediately
                    let death_sound_effect = if let Some(voice_index) =
                        crate::scripts::speech_util::resolve_entity_voice_index(world, entity_id)
                    {
                        // Randomly choose between loud and soft death sound
                        let concept = if rand::random::<bool>() {
                            "comdieloud".to_string()
                        } else {
                            "comdiesoft".to_string()
                        };

                        Effect::PlaySpeech {
                            entity_id,
                            voice_index,
                            concept,
                            tags: vec![],
                        }
                    } else {
                        Effect::NoEffect
                    };

                    let death_animation = Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: vec![MotionQueryItem::new("crumple")],
                        selection_strategy: dark::motion::MotionQuerySelectionStrategy::Random,
                    };

                    Effect::combine(vec![death_sound_effect, death_animation])
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
                    let is_locomotion = self.current_behavior.borrow().is_locomotion();
                    let selection_strategy = self.next_selection(is_locomotion);
                    Effect::QueueAnimationBySchema {
                        entity_id,
                        motion_query_items: self.current_behavior.borrow().animation(),
                        selection_strategy,
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

fn player_is_within_watch_obj(world: &World, entity_id: EntityId, radius: f32) -> bool {
    let u_player = world.borrow::<shipyard::UniqueView<PlayerInfo>>().unwrap();
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(ent_pos) = v_current_pos.get(entity_id) {
        return ent_pos.position.distance(u_player.pos) <= radius;
    }

    false
}
