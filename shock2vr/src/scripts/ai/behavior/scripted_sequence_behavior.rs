use std::cell::RefCell;

use cgmath::Deg;
use dark::{
    motion::MotionQueryItem,
    properties::{AIScriptedAction, AIScriptedActionType},
};
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        ai::steering::{Steering, SteeringOutput},
        script_util, Effect, Message,
    },
    time::Time,
};

use super::{Behavior, NoopBehavior, WanderBehavior};

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
        _entity_id: shipyard::EntityId,
    ) -> super::NextBehavior {
        if self.current_scripted_action.borrow().is_complete(world) {
            if self.current_action_idx >= ((self.actions.len() as i32) - 1) {
                println!("!!debug -next behavior, no opinion");
                super::NextBehavior::NoOpinion
            } else {
                let outgoing_effect = self.current_scripted_action.borrow().completion_effect();
                self.current_action_idx += 1;
                println!("!!debug -next behavior, now at {}", self.current_action_idx);
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
    println!("!!debug - checking action: {:?}", action);
    let current_behavior: Box<RefCell<dyn ScriptedAction>> = match &action.action_type {
        AIScriptedActionType::Play(action_name) => Box::new(RefCell::new(
            PlayAnimationScriptedAction::new(action_name.clone()),
        )),
        AIScriptedActionType::Frob(entity_name) => {
            Box::new(RefCell::new(FrobScriptedAction::new(world, entity_name)))
        }
        _ => Box::new(RefCell::new(IdleScriptedAction)),
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

    fn initial_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn completion_effect(&self) -> Effect {
        Effect::NoEffect
    }

    fn is_complete(&self, world: &World) -> bool {
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
    fn animation(self: &PlayAnimationScriptedAction) -> Vec<MotionQueryItem> {
        println!("!!debug: playing animation: {}", self.animation_name);

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
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }
}

pub struct FrobScriptedAction(Option<EntityId>);

impl FrobScriptedAction {
    pub fn new(world: &World, entity_name: &str) -> FrobScriptedAction {
        let maybe_entity = script_util::get_first_entity_by_name(world, entity_name);
        println!("!!debug - maybe entity: {:?}", maybe_entity);
        FrobScriptedAction(maybe_entity)
    }
}

impl ScriptedAction for FrobScriptedAction {
    fn animation(self: &FrobScriptedAction) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("__NULL_ANIMATION__")]
    }

    fn completion_effect(&self) -> Effect {
        if let Some(entity_id) = self.0 {
            println!("!!debug - sending frob to: {:?}", entity_id);
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
