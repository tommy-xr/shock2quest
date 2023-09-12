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

// SavePositionCommand
#[derive(Debug)]
pub struct SavePositionCommand {}

impl SavePositionCommand {
    pub fn new() -> SavePositionCommand {
        SavePositionCommand {}
    }
}

impl Command for SavePositionCommand {
    fn execute(&self, _world: &World) -> Effect {
        // let (save_data, _) = save_load::to_save_data(world);

        // let save_data_json = serde_json::to_string(&save_data).unwrap();

        // let zip_file = OpenOptions::new()
        //     .write(true)
        //     .create(true)
        //     .truncate(true)
        //     .open("save.sav")
        //     .unwrap();
        // let mut zip = ZipWriter::new(zip_file);

        // // Add first entry
        // let options = FileOptions::default()
        //     .compression_method(CompressionMethod::Deflated) // Choose your compression method
        //     .unix_permissions(0o755); // And the permissions of the file

        // zip.start_file("metadata.json", options).unwrap();
        // zip.write_all(save_data_json.as_bytes()).unwrap();

        // // Don't forget to finish the zip!
        // zip.finish().unwrap();
        Effect::GlobalEffect(crate::scripts::GlobalEffect::Save {
            file_name: "save1.sav".to_owned(),
        })
        // Effect::GlobalEffect(crate::scripts::GlobalEffect::TestReload)
        //Effect::NoEffect
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
        })
    }
}
