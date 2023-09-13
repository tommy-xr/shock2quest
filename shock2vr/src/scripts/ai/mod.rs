pub mod ai_util;
use ai_util::*;

pub mod steering;

mod animated_monster_ai;
pub use animated_monster_ai::*;

use std::{cell::RefCell, rc::Rc};

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3,
};
use dark::{
    motion::{MotionFlags, MotionQueryItem},
    properties::PropPosition,
    SCALE_FACTOR,
};
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
    time::Time,
    util::{
        get_position_from_matrix, get_position_from_transform, get_rotation_from_forward_vector,
        get_rotation_from_matrix, get_rotation_from_transform,
    },
};

use self::steering::*;

use super::{Effect, Message, MessagePayload, Script};

pub enum NextBehavior {
    NoOpinion,
    Next(Box<RefCell<dyn Behavior>>),
    Stay,
}

pub trait AI: Script {}

pub trait Behavior {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![]
    }

    ///
    /// turn_speed
    ///
    /// Turn speed of the character in degrees / s
    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }

    fn next_behavior(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        NextBehavior::NoOpinion
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}

pub struct NoopBehavior;

impl Behavior for NoopBehavior {}

pub struct IdleBehavior;

impl Behavior for IdleBehavior {
    fn animation(self: &IdleBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
        //vec![MotionQueryItem::new("stand")]
    }
}

pub struct WanderBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl WanderBehavior {
    pub fn new() -> WanderBehavior {
        WanderBehavior {
            steering_strategy: Box::new(CollisionAvoidanceSteeringStrategy::comprehensive()),
        }
    }
}

impl Behavior for WanderBehavior {
    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn animation(self: &WanderBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::with_value("direction", 0).optional(),
            MotionQueryItem::new("locomote"),
            //MotionQueryItem::new("search").optional(),
        ]
    }
}

pub struct DeadBehavior {}

impl Behavior for DeadBehavior {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(0.0)
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }

    fn animation(self: &DeadBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("crumple").optional()]
    }
}

pub struct ChaseBehavior {
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl ChaseBehavior {
    pub fn new() -> ChaseBehavior {
        ChaseBehavior {
            steering_strategy: steering::chained(vec![
                Box::new(
                    CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
                ),
                Box::new(ChasePlayerSteeringStrategy),
            ]),
        }
    }
}

impl Behavior for ChaseBehavior {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(360.0)
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
    }

    fn animation(self: &ChaseBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn next_behavior(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        let rand = rand::thread_rng().gen_range(0..100);
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();
        //let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();

        let melee_attack_distance = 8.0 / SCALE_FACTOR;
        let ranged_max_attack_distance = 40.0 / SCALE_FACTOR;
        let ranged_min_attack_distance = 15.0 / SCALE_FACTOR;

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            let distance = (prop_pos.position - u_player.pos).magnitude();

            // if distance > ranged_min_attack_distance && distance < ranged_max_attack_distance {
            //     return NextBehavior::Next(Box::new(RefCell::new(RangedAttackBehavior)));
            // }
            if distance < melee_attack_distance {
                return NextBehavior::Next(Box::new(RefCell::new(MeleeAttackBehavior)));
            }
        }

        NextBehavior::Stay
    }
}

pub struct SearchBehavior;

impl Behavior for SearchBehavior {
    fn animation(self: &SearchBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("search"),
            MotionQueryItem::new("scan").optional(),
        ]
    }
}

pub struct MeleeAttackBehavior;

impl Behavior for MeleeAttackBehavior {
    fn animation(self: &MeleeAttackBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("meleecombat"),
            MotionQueryItem::new("attack").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        ChasePlayerSteeringStrategy.steer(current_heading, world, physics, entity_id, time)
    }

    fn next_behavior(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

        let melee_attack_distance = 8.0 / SCALE_FACTOR;

        if let Ok(prop_pos) = v_current_pos.get(entity_id) {
            let distance = (prop_pos.position - u_player.pos).magnitude();

            if distance < melee_attack_distance {
                return NextBehavior::Stay;
            }
        }
        NextBehavior::Next(Box::new(RefCell::new(ChaseBehavior::new())))
    }
}

pub struct RangedAttackBehavior;

impl Behavior for RangedAttackBehavior {
    fn animation(self: &RangedAttackBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("rangedcombat").optional(),
            MotionQueryItem::new("attack").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        ChasePlayerSteeringStrategy.steer(current_heading, world, physics, entity_id, time)
    }

    fn next_behavior(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> NextBehavior {
        NextBehavior::Next(Box::new(RefCell::new(ChaseBehavior::new())))
    }
}

pub struct DieBehavior;

impl Behavior for DieBehavior {
    fn animation(self: &DieBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("crumple").optional()]
    }
}

pub fn random_behavior() -> Box<RefCell<dyn Behavior>> {
    let mut potential_behaviors: Vec<Box<RefCell<dyn Behavior>>> = vec![
        // Rc::new(MeleeAttackBehavior),
        // Rc::new(SearchBehavior),
        Box::new(RefCell::new(WanderBehavior::new())),
        Box::new(RefCell::new(WanderBehavior::new())),
        Box::new(RefCell::new(WanderBehavior::new())),
        //Rc::new(IdleBehavior),
        // Rc::new(RangedAttackBehavior),
        //Rc::new(ChaseBehavior),
        //Rc::new(DieBehavior),
    ];
    let mut rng = rand::thread_rng();
    let idx = rng.gen_range(0..potential_behaviors.len());
    potential_behaviors.remove(idx)
}
