///
/// empty_entity_populator.rs
///
/// An implementation of EntityPopulator that doesn't populate any entities
use shipyard::World;
use std::collections::HashMap;

use dark::mission::SystemShock2Level;
use dark::properties::WrappedEntityId;
use dark::ss2_entity_info::SystemShock2EntityInfo;

use super::EntityPopulator;

#[allow(dead_code)]
pub struct EmptyEntityPopulator {}

impl EmptyEntityPopulator {}

impl EntityPopulator for EmptyEntityPopulator {
    fn populate(
        &self,
        _gamesys_entity_info: &SystemShock2EntityInfo,
        _level: &SystemShock2Level,
        _world: &mut World,
    ) -> HashMap<i32, WrappedEntityId> {
        HashMap::new()
    }
}
