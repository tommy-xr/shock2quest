use crate::engine::EngineRenderContext;
use crate::scene::light::LightArray;
use cgmath::{Matrix, Matrix3, Matrix4, SquareMatrix, Vector3, Vector4};
use std::any::Any;

/// A colour added over lighting for one draw, limited to a texture-space rect
/// (min u, min v, max u, max v) so one part of a shared atlas can glow alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub color: Vector3<f32>,
    pub uv_rect: Vector4<f32>,
}

impl Glow {
    pub const NONE: Glow = Glow::everywhere(Vector3::new(0.0, 0.0, 0.0));
    /// A rect bound past any real UV, yet inside GLES mediump range (~6.5e4).
    pub const UNBOUNDED: f32 = 1.0e4;

    pub const fn everywhere(color: Vector3<f32>) -> Glow {
        Glow {
            color,
            uv_rect: Vector4::new(
                -Glow::UNBOUNDED,
                -Glow::UNBOUNDED,
                Glow::UNBOUNDED,
                Glow::UNBOUNDED,
            ),
        }
    }
}

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

    /// Self-illumination inherited by shaded authored layers.
    fn emissivity(&self) -> f32 {
        0.0
    }

    /// Multiply the authored emissivity for the next draw (1.0 = authored).
    /// Materials are shared across entities, so callers reset it afterwards.
    fn set_emissivity_scale(&mut self, _scale: f32) {}

    /// Draw at full texture intensity, ignoring scene lighting, for the next
    /// draw (Dark's FullBright render type). Shared like the emissivity
    /// scale, so callers reset it afterwards.
    fn set_full_bright(&mut self, _full_bright: bool) {}

    /// Add a glow over lighting for the next draw (`Glow::NONE` = none).
    /// Shared like the emissivity scale, so callers reset it afterwards.
    fn set_glow(&mut self, _glow: Glow) {}

    /// Replace diffuse only after a complete material plan has loaded.
    fn set_diffuse_texture(&mut self, _texture: std::rc::Rc<dyn crate::texture::TextureTrait>) {}

    /// Configure an authored render pass; other material types keep their own path.
    fn set_render_pass(&mut self, _pass: crate::scene::render_pass::RenderPass) {}

    /// Composite an authored shine over this material in the same draw.
    /// Default is a no-op for materials that do not light a diffuse.
    fn set_shine(&mut self, _shine: crate::scene::shine::Shine) {}

    /// The transparency currently in effect (0.0 = opaque, 1.0 = invisible),
    /// for debug inspection only. `None` for materials without transparency.
    fn transparency(&self) -> Option<f32> {
        None
    }
}

/// Multiplier on the *lit* colour of a glowing material under an emissivity
/// scale, so a dimmed glow part reads as an unpowered lamp rather than a
/// cyan surface lit by the room. Non-glowing materials are never darkened.
pub fn lit_scale(emissivity: f32, emissivity_scale: f32) -> f32 {
    /// Lit colour left on a fully unpowered glow part.
    const UNPOWERED: f32 = 0.15;
    if emissivity > 0.0 {
        UNPOWERED + (1.0 - UNPOWERED) * emissivity_scale.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The (emission, lit scale) a shader takes: full bright shows the texture
/// as-is, all emission and no light (an authored shine still takes scene
/// light); otherwise the authored emission under its scale (see `lit_scale`).
pub fn emission_and_lit_scale(
    emissivity: f32,
    emissivity_scale: f32,
    full_bright: bool,
) -> (f32, f32) {
    if full_bright {
        (1.0, 0.0)
    } else {
        (
            emissivity * emissivity_scale,
            lit_scale(emissivity, emissivity_scale),
        )
    }
}

/// World-space position of the eye a view matrix looks from.
pub fn eye_position(view: &Matrix4<f32>) -> cgmath::Vector3<f32> {
    view.invert().unwrap_or_else(Matrix4::identity).w.truncate()
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

    /// Full bright shows the texture as-is whatever the authored glow: all
    /// emission, no scene light. Otherwise the authored terms pass through.
    #[test]
    fn full_bright_is_all_emission_and_no_light() {
        for (emissivity, scale) in [(0.0, 1.0), (0.5, 0.2), (1.0, 1.0)] {
            assert_eq!(emission_and_lit_scale(emissivity, scale, true), (1.0, 0.0));
            assert_eq!(
                emission_and_lit_scale(emissivity, scale, false),
                (emissivity * scale, lit_scale(emissivity, scale))
            );
        }
    }

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

    #[test]
    fn only_glow_parts_darken_with_their_glow() {
        assert_eq!(lit_scale(0.0, 0.0), 1.0);
        assert_eq!(lit_scale(1.0, 1.0), 1.0);
        assert!(lit_scale(1.0, 0.0) < 0.2);
    }
}
