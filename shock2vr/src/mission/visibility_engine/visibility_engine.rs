use std::{
    collections::{HashMap, HashSet},
    ffi::IntoStringError,
};

use cgmath::{frustum, point2, vec2, vec3, Matrix4, Point3, SquareMatrix, Vector3};
use collision::{Aabb2, Contains, Continuous, Discrete, Frustum, Relation, Union};
use dark::{
    importers::TEXTURE_IMPORTER,
    mission::{Cell, CellPortal, SystemShock2Level},
    properties::PropPosition,
};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPosition},
    texture::TextureOptions,
};
use shipyard::{EntityId, IntoIter, IntoWithId, View, World};

use super::CullingInfo;

pub trait VisibilityEngine {
    fn prepare(&mut self, level: &SystemShock2Level, world: &World, culling_info: &CullingInfo) {}

    fn is_visible(&mut self, entity_id: EntityId) -> bool;

    fn debug_render(&self, asset_cache: &mut AssetCache) -> Vec<SceneObject> {
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
