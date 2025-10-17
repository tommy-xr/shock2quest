use std::{cell::RefCell, time::Duration};

use cgmath::{vec3, Deg, InnerSpace};
use dark::{
    motion::MotionQueryItem,
    properties::{AIScriptedAction, AIScriptedActionType, PropPosition},
    SCALE_FACTOR,
};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        ai::{
            ai_util,
            steering::{
                self, ChaseEntitySteeringStrategy,
                CollisionAvoidanceSteeringStrategy, Steering, SteeringOutput, SteeringStrategy,
            },
        },
        script_util, Effect, Message,
    },
    time::Time,
};

use super::Behavior;

pub struct ScriptedSequenceBehavior {
    actions: Vec<AIScriptedAction>,
    queued_effects: Vec<Effect>,
    current_action_idx: i32,
    current_scripted_action: Box<RefCell<dyn ScriptedAction>>,
}

impl ScriptedSequenceBehavior {
    pub fn new(world: &World, actions: Vec<AIScriptedAction>) -> ScriptedSequenceBehavior {
        let current_behavior = get_behavior_from_action(world, &actions[0]);
        let initial_effect = current_behavior.borrow().initial_effect();

        ScriptedSequenceBehavior {
            actions,
            queued_effects: vec![initial_effect],
            current_action_idx: 0,
            current_scripted_action: current_behavior,
        }
    }
}

impl Behavior for ScriptedSequenceBehavior {
    fn animation(&self) -> Vec<MotionQueryItem> {
        self.current_scripted_action.borrow().animation()
    }

    fn turn_speed(&self) -> Deg<f32> {
        self.current_scripted_action.borrow().turn_speed()
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let queued_effects = Effect::combine(self.queued_effects.clone());
        self.queued_effects = vec![];

        let maybe_output = self.current_scripted_action.borrow_mut().update(
            current_heading,
            world,
            physics,
            entity_id,
            time,
        );

        if let Some((steering_output, eff)) = maybe_output {
            Some((steering_output, Effect::combine(vec![queued_effects, eff])))
        } else {
            Some((Steering::from_current(current_heading), queued_effects))
        }
    }

    fn next_behavior(
        &mut self,
        world: &shipyard::World,
        _physics: &crate::physics::PhysicsWorld,
        entity_id: shipyard::EntityId,
    ) -> super::NextBehavior {
        if self
            .current_scripted_action
            .borrow()
            .is_complete(entity_id, world)
        {
            if self.current_action_idx >= ((self.actions.len() as i32) - 1) {
                super::NextBehavior::NoOpinion
            } else {
                let outgoing_effect = self.current_scripted_action.borrow().completion_effect();
                self.current_action_idx += 1;
                let behavior = get_behavior_from_action(
                    world,
                    &self.actions[self.current_action_idx as usize],
                );
                self.current_scripted_action = behavior;
                let incoming_effect = self.current_scripted_action.borrow().initial_effect();

                // Queue up effects from the behavior
                self.queued_effects.push(incoming_effect);
                self.queued_effects.push(outgoing_effect);

                super::NextBehavior::Stay
            }
        } else {
            super::NextBehavior::Stay
        }
    }
}

fn get_behavior_from_action(
    world: &World,
    action: &AIScriptedAction,
) -> Box<RefCell<dyn ScriptedAction>> {
    let current_behavior: Box<RefCell<dyn ScriptedAction>> = match &action.action_type {
        AIScriptedActionType::Play(action_name) => Box::new(RefCell::new(
            PlayAnimationScriptedAction::new(action_name.clone()),
        )),
        AIScriptedActionType::Face { entity_name } => {
            Box::new(RefCell::new(FaceScriptedAction::new(world, &entity_name)))
        }
        AIScriptedActionType::Frob(entity_name) => {
            Box::new(RefCell::new(FrobScriptedAction::new(world, entity_name)))
        }
        AIScriptedActionType::Goto {
            waypoint_name,
            speed: _, // TODO: Incorporate speed
        } => Box::new(RefCell::new(GotoScriptedAction::new(world, &waypoint_name))),

        AIScriptedActionType::Wait(duration) => {
            Box::new(RefCell::new(WaitScriptedAction::new(*duration)))
        }
        _ => Box::new(RefCell::new(NoopScriptedAction)),
    };
    current_behavior
}

/// ScriptedAction
/// animation:
/// update (&mut self, etc)
/// handle_message
/// is_complete

trait ScriptedAction {
    fn animation(&self) -> Vec<MotionQueryItem> {
        vec![]
    }

    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }

    fn initial_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn completion_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn is_complete(&self, _entity_id: EntityId, _world: &World) -> bool {
        true
    }

    fn update(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }
}

pub struct PlayAnimationScriptedAction {
    animation_name: String,
}

impl PlayAnimationScriptedAction {
    pub fn new(animation_name: String) -> PlayAnimationScriptedAction {
        PlayAnimationScriptedAction { animation_name }
    }
}

impl ScriptedAction for PlayAnimationScriptedAction {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(0.0)
    }
    fn animation(self: &PlayAnimationScriptedAction) -> Vec<MotionQueryItem> {
        if self.animation_name.find(",").is_some() {
            return self
                .animation_name
                .split(",")
                .map(|s| MotionQueryItem::new(s.to_ascii_lowercase().trim()))
                .collect::<Vec<MotionQueryItem>>();
        }

        if let Some(index) = self.animation_name.find(' ') {
            let (motion, value_str) = self.animation_name.split_at(index);
            if let Ok(value) = value_str.trim().parse::<i32>() {
                // I'm assuming the number is an i32, adjust as needed
                return vec![MotionQueryItem::with_value(motion, value)];
            }
        }

        vec![MotionQueryItem::new(&self.animation_name)]
    }
}

pub struct IdleScriptedAction;

impl ScriptedAction for IdleScriptedAction {
    fn animation(self: &IdleScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
    }
}

pub struct NoopScriptedAction;

impl ScriptedAction for NoopScriptedAction {
    fn animation(self: &NoopScriptedAction) -> Vec<MotionQueryItem> {
        // vec![MotionQueryItem::with_value("cs", 2)]
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }
}

pub struct WaitScriptedAction {
    remaining_duration_in_seconds: f32,
}

impl WaitScriptedAction {
    pub fn new(time: Duration) -> WaitScriptedAction {
        WaitScriptedAction {
            remaining_duration_in_seconds: time.as_secs_f32(),
        }
    }
}

impl ScriptedAction for WaitScriptedAction {
    fn animation(self: &WaitScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn update(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        self.remaining_duration_in_seconds -= time.elapsed.as_secs_f32();
        Some((Steering::from_current(current_heading), Effect::NoEffect))
    }

    fn is_complete(&self, _entity_id: EntityId, _world: &World) -> bool {
        self.remaining_duration_in_seconds <= 0.0
    }
}

pub struct FrobScriptedAction(Option<EntityId>);

impl FrobScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> FrobScriptedAction {
        let maybe_entity = script_util::get_first_entity_by_name(world, entity_name);
        FrobScriptedAction(maybe_entity)
    }
}

impl ScriptedAction for FrobScriptedAction {
    fn animation(self: &FrobScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn completion_effect(&self) -> Effect {
        if let Some(entity_id) = self.0 {
            Effect::Send {
                msg: Message {
                    to: entity_id,
                    payload: crate::scripts::MessagePayload::Frob,
                },
            }
        } else {
            Effect::NoEffect
        }
    }
}

pub struct GotoScriptedAction {
    target_id: Option<EntityId>,
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl GotoScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> GotoScriptedAction {
        let maybe_entity = script_util::get_first_entity_by_name(world, entity_name);

        let mut steering_strategies: Vec<Box<dyn SteeringStrategy>> = vec![Box::new(
            CollisionAvoidanceSteeringStrategy::conservative(), /* conservative so we can focus on the chase */
        )];

        if let Some(ent) = maybe_entity {
            steering_strategies.push(Box::new(ChaseEntitySteeringStrategy::new(ent)))
            //steering_strategies.push(Box::new(ChasePlayerSteeringStrategy))
        }

        GotoScriptedAction {
            target_id: maybe_entity,
            steering_strategy: steering::chained(steering_strategies),
        }
    }
}

impl ScriptedAction for GotoScriptedAction {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(540.0)
    }
    fn animation(self: &GotoScriptedAction) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("locomote"),
            MotionQueryItem::with_value("direction", 0).optional(),
            MotionQueryItem::new("locourgent").optional(),
        ]
    }
    fn update(
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

    fn is_complete(&self, entity_id: EntityId, world: &World) -> bool {
        let v_prop_pos = world.borrow::<View<PropPosition>>().unwrap();
        if let Some(target_entity_id) = self.target_id {
            if let Ok(target_pos) = v_prop_pos.get(target_entity_id) {
                if let Ok(entity_pos) = v_prop_pos.get(entity_id) {
                    let from = vec3(entity_pos.position.x, 0.0, entity_pos.position.z);
                    let to = vec3(target_pos.position.x, 0.0, target_pos.position.z);
                    let distance = (from - to).magnitude();

                    // HACK: This is an arbitrary value that I just tested with some sequences
                    // (ie, in rec1). I'm not sure the best criteria for this step yet.
                    return distance < (3.0 / SCALE_FACTOR);
                }
            }
        }

        true
    }
}

pub struct FaceScriptedAction {
    target_id: Option<EntityId>,
    steering_strategy: Box<dyn SteeringStrategy>,
}

impl FaceScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> FaceScriptedAction {
        let maybe_entity = script_util::get_first_entity_by_name(world, entity_name);

        let mut steering_strategies: Vec<Box<dyn SteeringStrategy>> = vec![];

        if let Some(ent) = maybe_entity {
            steering_strategies.push(Box::new(ChaseEntitySteeringStrategy::new(ent)))
        }

        FaceScriptedAction {
            target_id: maybe_entity,
            steering_strategy: steering::chained(steering_strategies),
        }
    }
}

impl ScriptedAction for FaceScriptedAction {
    fn turn_speed(&self) -> Deg<f32> {
        Deg(180.0)
    }
    fn animation(self: &FaceScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ACTION__")]
    }
    fn update(
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

    fn is_complete(&self, entity_id: EntityId, world: &World) -> bool {
        let v_prop_pos = world.borrow::<View<PropPosition>>().unwrap();
        if let Some(target_entity_id) = self.target_id {
            if let Ok(target_pos) = v_prop_pos.get(target_entity_id) {
                if let Ok(entity_pos) = v_prop_pos.get(entity_id) {
                    let current_yaw = ai_util::current_yaw(entity_id, world);
                    let yaw_between_vectors =
                        ai_util::yaw_between_vectors(entity_pos.position, target_pos.position);

                    let delta = (current_yaw - yaw_between_vectors).0.abs();

                    return delta < 1.0;
                }
            }
        }

        true
    }
}
