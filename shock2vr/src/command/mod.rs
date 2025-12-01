mod move_inventory_command;
mod spawn_item_command;

pub use move_inventory_command::*;
use shipyard::World;
pub use spawn_item_command::*;

use std::fmt;

use crate::scripts::Effect;

pub trait Command: fmt::Debug {
    fn execute(&self, world: &World) -> Effect;
}

// SaveCommand
#[derive(Debug)]
pub struct SaveCommand {}

impl SaveCommand {
    pub fn new() -> SaveCommand {
        SaveCommand {}
    }
}

impl Command for SaveCommand {
    fn execute(&self, _world: &World) -> Effect {
        Effect::GlobalEffect(crate::scripts::GlobalEffect::Save {
            file_name: "save1.sav".to_owned(),
        })
    }
}

// LoadCommand
#[derive(Debug)]
pub struct LoadCommand {}

impl LoadCommand {
    pub fn new() -> LoadCommand {
        LoadCommand {}
    }
}

impl Command for LoadCommand {
    fn execute(&self, _world: &World) -> Effect {
        Effect::GlobalEffect(crate::scripts::GlobalEffect::Load {
            file_name: "save1.sav".to_owned(),
        })
    }
}

#[derive(Debug)]
pub struct TransitionLevelCommand {}

impl TransitionLevelCommand {
    pub fn new() -> TransitionLevelCommand {
        TransitionLevelCommand {}
    }
}

impl Command for TransitionLevelCommand {
    fn execute(&self, _world: &World) -> Effect {
        Effect::GlobalEffect(crate::scripts::GlobalEffect::TransitionLevel {
            level_file: "medsci1.mis".to_owned(),
            loc: None,
            entities_to_trigger: vec![],
        })
    }
}

// PathfindingTestCommand
#[derive(Debug)]
pub struct PathfindingTestCommand {}

impl PathfindingTestCommand {
    pub fn new() -> PathfindingTestCommand {
        PathfindingTestCommand {}
    }
}

impl Command for PathfindingTestCommand {
    fn execute(&self, _world: &World) -> Effect {
        Effect::PathfindingTest
    }
}
