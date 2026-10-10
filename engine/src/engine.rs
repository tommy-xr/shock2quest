#[derive(Clone, Copy)]
pub struct EngineRenderContext {
    pub time: f32,

    /// Per-object visual displacement, installed by SceneObject at draw time.
    pub world_wave: crate::scene::world_wave::WorldWave,

    /// Renderer-owned self-occlusion mask; callers start with None.
    pub self_depth: crate::scene::self_depth::Phase,

    /// Multiplier for world and model ambient lighting; 1 preserves it.
    pub ambient_light_intensity: f32,
    /// Multiplier for baked world lightmaps, applied before the ambient floor.
    pub level_light_intensity: f32,
    /// Opt-in light-driven gloss on selected upgraded Many terrain.
    pub terrain_wetness: f32,

    pub camera_offset: cgmath::Vector3<f32>,
    pub camera_rotation: cgmath::Quaternion<f32>,

    pub head_offset: cgmath::Vector3<f32>,
    pub head_rotation: cgmath::Quaternion<f32>,

    pub projection_matrix: cgmath::Matrix4<f32>,

    pub screen_size: cgmath::Vector2<f32>,
}

use crate::file_system::Storage;
use crate::scene::scene::Scene;
use std::sync::Arc;

pub trait Engine {
    fn render(&self, render_context: &EngineRenderContext, scene: &Scene);

    fn get_storage(&self) -> Arc<dyn Storage>;
}
