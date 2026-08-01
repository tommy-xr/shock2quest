use dark::properties::WrappedEntityId;
use shipyard::{EntityId, World};
use std::collections::HashMap;

use crate::scripts::SavedScriptState;
use dark::ss2_entity_info::SystemShock2EntityInfo;

pub struct EntityPopulation {
    pub template_to_entity_id: HashMap<i32, WrappedEntityId>,
    /// Mapping from pre-save IDs to this world's IDs. Empty for a fresh
    /// mission population.
    pub entity_id_map: HashMap<EntityId, EntityId>,
    pub script_states: Vec<SavedScriptState>,
}

pub trait EntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level_entity_info: &SystemShock2EntityInfo,
        obj_name_map: &HashMap<i32, String>, // name override map
        world: &mut World,
    ) -> EntityPopulation;
}
