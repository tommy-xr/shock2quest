use crate::engine::EngineRenderContext;
use crate::scene::light::LightArray;
use cgmath::{Matrix, Matrix3, Matrix4, SquareMatrix};
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

    /// Override the material's transparency (0.0 = opaque, 1.0 = invisible), or
    /// reset to its authored value with `None`. Default is a no-op for
    /// materials without transparency support.
    fn set_transparency_override(&mut self, _transparency: Option<f32>) {}

    /// Composite an authored shine over this material in the same draw.
    /// Default is a no-op for materials that do not light a diffuse.
    fn set_shine(&mut self, _shine: crate::scene::shine::Shine) {}

    /// The transparency currently in effect (0.0 = opaque, 1.0 = invisible),
    /// for debug inspection only. `None` for materials without transparency.
    fn transparency(&self) -> Option<f32> {
        None
    }
}

/// Maps object-space normals to world space: the inverse-transpose of the
/// world matrix's 3x3, so normals stay perpendicular under non-uniform scale.
/// Computed once per draw rather than per vertex in the shader.
pub fn normal_matrix(world: &Matrix4<f32>) -> Matrix3<f32> {
    let linear = Matrix3::from_cols(world.x.truncate(), world.y.truncate(), world.z.truncate());
    linear
        .invert()
        .map_or(linear, |inverse| inverse.transpose())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Vector3};

    /// A 45-degree slope squashed 4x along x: its normal must stay
    /// perpendicular to the squashed surface.
    #[test]
    fn normal_matrix_keeps_normals_perpendicular_under_non_uniform_scale() {
        let world = Matrix4::from_nonuniform_scale(4.0, 1.0, 1.0);
        let tangent = Vector3::new(1.0, 1.0, 0.0);
        let normal = Vector3::new(1.0, -1.0, 0.0);

        let world_tangent = (world * tangent.extend(0.0)).truncate();
        let world_normal = normal_matrix(&world) * normal;

        assert!(world_tangent.dot(world_normal).abs() < 1e-5);
    }
}
