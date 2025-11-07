use crate::engine::EngineRenderContext;
use crate::scene::light::LightArray;
use cgmath::Matrix4;
use std::any::Any;

pub trait Material: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn has_initialized(&self) -> bool;
    fn initialize(&mut self, is_opengl_es: bool);

    /// Draw opaque material with single-pass lighting
    ///
    /// This method renders the material with all lighting calculations performed in a single pass.
    /// The lights parameter contains up to 6 spotlights that will be processed in the shader.
    ///
    /// Parameters:
    /// - render_context: Engine rendering context
    /// - view_matrix: Camera view matrix
    /// - world_matrix: Object world transformation matrix
    /// - skinning_data: Bone matrices for skinned meshes
    /// - lights: Array of up to 6 spotlights for lighting calculations
    ///
    /// Returns: true if the material rendered something, false otherwise
    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        lights: &LightArray,
    ) -> bool;

    /// Draw transparent material with single-pass lighting
    ///
    /// Similar to draw_opaque but for transparent materials that need special blending.
    fn draw_transparent(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &LightArray,
    ) -> bool {
        false
    }
}
