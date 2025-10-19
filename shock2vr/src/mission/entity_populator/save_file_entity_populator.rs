/**
 * save_file_entity_populator.rs
 *
 * An implementation of EntityPopulator that creates entities based on the entity data in save file.
 *
 */
use dark::properties::WrappedEntityId;
use shipyard::World;
use std::collections::HashMap;

use dark::mission::SystemShock2Level;
use dark::ss2_entity_info::SystemShock2EntityInfo;

use crate::save_load::EntitySaveData;

use super::EntityPopulator;

pub struct SaveFileEntityPopulator {
    pub save_data: EntitySaveData,
}

impl<'a> SaveFileEntityPopulator {
    pub fn create(save_data: EntitySaveData) -> SaveFileEntityPopulator {
        SaveFileEntityPopulator { save_data }
    }
}

impl EntityPopulator for SaveFileEntityPopulator {
    fn populate(
        &self,
        _gamesys_entity_info: &SystemShock2EntityInfo,
        _level: &SystemShock2Level,
        world: &mut World,
    ) -> HashMap<i32, WrappedEntityId> {
        // panic!("todo: implement save file entity populator");

        let world_entity_data = &self.save_data;
        let (template_to_entity, _) = world_entity_data.instantiate(world);
        template_to_entity
    }
}
