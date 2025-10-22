use crate::engine::EngineRenderContext;
use crate::scene::light_system::LightingBatch;
use cgmath::Matrix4;

pub trait Material {
    fn has_initialized(&self) -> bool;
    fn initialize(&mut self, is_opengl_es: bool, storage: &dyn crate::file_system::Storage);
    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        lighting: &LightingBatch,
    ) -> bool;
    fn draw_transparent(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
    ) -> bool {
        false
    }
}
