pub struct EngineRenderContext {
    pub time: f32,

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
