use std::cell::RefCell;

use dark::{
    motion::MotionQueryItem,
    properties::{AIScriptedAction, AIScriptedActionType},
};

use super::{Behavior, NoopBehavior, PlayAnimationBehavior, WanderBehavior};

pub struct ScriptedSequenceBehavior {
    actions: Vec<AIScriptedAction>,
    current_action_idx: i32,
    current_behavior: Box<RefCell<dyn Behavior>>,
}

impl ScriptedSequenceBehavior {
    pub fn new(actions: Vec<AIScriptedAction>) -> ScriptedSequenceBehavior {
        let current_behavior = get_behavior_from_action(&actions[0]);

        ScriptedSequenceBehavior {
            actions,
            current_action_idx: 0,
            current_behavior,
        }
    }
}

impl Behavior for ScriptedSequenceBehavior {
    fn animation(&self) -> Vec<MotionQueryItem> {
        self.current_behavior.borrow().animation()
    }

    fn next_behavior(
        &mut self,
        _world: &shipyard::World,
        _physics: &crate::physics::PhysicsWorld,
        _entity_id: shipyard::EntityId,
    ) -> super::NextBehavior {
        if self.current_action_idx >= ((self.actions.len() as i32) - 1) {
            println!("!!debug -next behavior, no opinion");
            super::NextBehavior::NoOpinion
        } else {
            self.current_action_idx += 1;
            println!("!!debug -next behavior, now at {}", self.current_action_idx);
            let behavior: Box<RefCell<dyn Behavior>> =
                get_behavior_from_action(&self.actions[self.current_action_idx as usize]);
            self.current_behavior = behavior;
            super::NextBehavior::Stay
        }
    }
}

fn get_behavior_from_action(action: &AIScriptedAction) -> Box<RefCell<dyn Behavior>> {
    println!("!!debug - checking action: {:?}", action);
    let current_behavior: Box<RefCell<dyn Behavior>> = match &action.action_type {
        AIScriptedActionType::Play(action_name) => Box::new(RefCell::new(
            PlayAnimationBehavior::new(action_name.clone()),
        )),
        _ => Box::new(RefCell::new(super::IdleBehavior)),
    };
    current_behavior
}
