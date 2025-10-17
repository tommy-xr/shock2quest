use crate::engine::EngineRenderContext;
use crate::scene::light::Light;
use cgmath::Matrix4;

pub trait Material {
    fn has_initialized(&self) -> bool;
    fn initialize(&mut self, is_opengl_es: bool, storage: &Box<dyn crate::file_system::Storage>);
    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
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

    /// Draw material with lighting pass for multi-pass lighting system
    ///
    /// This method is called once per light that affects the geometry.
    /// The material should render with additive blending to accumulate light contributions.
    ///
    /// Parameters:
    /// - render_context: Engine rendering context
    /// - view_matrix: Camera view matrix
    /// - world_matrix: Object world transformation matrix
    /// - skinning_data: Bone matrices for skinned meshes
    /// - light: The light to render with
    /// - shadow_map: Optional shadow map texture (future extension)
    ///
    /// Returns: true if the material rendered something, false otherwise
    fn draw_light_pass(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _light: &dyn Light,
        _shadow_map: Option<&()>, // Placeholder for future ShadowMap type
    ) -> bool {
        // Default implementation: no lighting support
        false
    }
}
