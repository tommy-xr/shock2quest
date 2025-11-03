use dark::properties::WrappedEntityId;
use shipyard::World;
use std::collections::HashMap;

use dark::ss2_entity_info::SystemShock2EntityInfo;
pub trait EntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level_entity_info: &SystemShock2EntityInfo,
        obj_name_map: &HashMap<i32, String>, // name override map
        world: &mut World,
    ) -> HashMap<i32, WrappedEntityId>;
}
