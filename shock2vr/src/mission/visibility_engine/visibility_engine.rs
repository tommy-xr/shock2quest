use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, World};

use super::CullingInfo;
use crate::mission::SpatialQueryEngine;

pub trait VisibilityEngine {
    fn prepare(
        &mut self,
        _spatial_data: Option<&dyn SpatialQueryEngine>,
        _world: &World,
        _culling_info: &CullingInfo,
    ) {
    }

    fn is_visible(&mut self, entity_id: EntityId) -> bool;

    /// Cull a second view (the psi portal's remote camera), starting from the
    /// fallback position's cell when its eye is outside every cell. `None`
    /// drops it, so `is_visible_secondary` mirrors the main view.
    fn prepare_secondary(
        &mut self,
        _spatial_data: Option<&dyn SpatialQueryEngine>,
        _world: &World,
        _view: Option<(&CullingInfo, cgmath::Vector3<f32>)>,
    ) {
    }

    fn is_visible_secondary(&mut self, entity_id: EntityId) -> bool {
        self.is_visible(entity_id)
    }

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
