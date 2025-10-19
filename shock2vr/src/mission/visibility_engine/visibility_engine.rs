use dark::mission::SystemShock2Level;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, World};

use super::CullingInfo;

pub trait VisibilityEngine {
    fn prepare(&mut self, _level: &SystemShock2Level, _world: &World, _culling_info: &CullingInfo) {
    }

    fn is_visible(&mut self, entity_id: EntityId) -> bool;

    fn debug_render(&self, _asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        Vec::new()
    }
}

pub struct AlwaysVisible;

impl VisibilityEngine for AlwaysVisible {
    fn is_visible(&mut self, _entity_id: EntityId) -> bool {
        true
    }
}

pub struct NeverVisible;

impl VisibilityEngine for NeverVisible {
    fn is_visible(&mut self, _entity_id: EntityId) -> bool {
        false
    }
}
