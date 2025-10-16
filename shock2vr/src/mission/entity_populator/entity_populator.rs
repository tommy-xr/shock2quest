use dark::properties::WrappedEntityId;
use shipyard::World;
use rustc_hash::FxHashMap;

use dark::mission::SystemShock2Level;
use dark::ss2_entity_info::SystemShock2EntityInfo;
pub trait EntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level: &SystemShock2Level,
        world: &mut World,
    ) -> FxHashMap<i32, WrappedEntityId>;
}
