extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use c_string::*;
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3};
use once_cell::sync::OnceCell;
use std::any::Any;

/// A flat quad that is transparent in the middle and tinted at the rim.
///
/// The falloff is computed per-fragment from the quad's own UVs rather than
/// baked into a texture, because the layer this material exists for is a
/// *view-locked* quad: it is sized by an angular ratio, so where the rim lands
/// is a property of the geometry, and the two radii below are the only knobs.
/// A texture would have to be regenerated whenever those radii changed, and a
/// textured world-space material in this engine either alpha-*cuts* (see
/// `basic_material`'s `discard`) or is screen-space only - neither of which can
/// draw a soft ramp in the world.
const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec2 texCoord;

        void main() {
            texCoord = inTex;
            gl_Position = projection * view * world * vec4(inPos, 1.0);
        }
"#;

/// `radius` is 0 at the quad's centre and 1 at the middle of each edge (so it
/// keeps going past 1 into the corners, which stay fully tinted).
const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;

        uniform vec3 color;
        uniform float intensity;
        uniform float innerRadius;
        uniform float outerRadius;

        void main() {
            float radius = length(texCoord - vec2(0.5, 0.5)) * 2.0;
            float span = max(outerRadius - innerRadius, 0.0001);
            float t = clamp((radius - innerRadius) / span, 0.0, 1.0);
            // Smoothstep, so the rim has no visible banding edge where it
            // meets the clear centre.
            float ramp = t * t * (3.0 - 2.0 * t);
            fragColor = vec4(color, ramp * intensity);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    color_loc: i32,
    intensity_loc: i32,
    inner_radius_loc: i32,
    outer_radius_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct VignetteMaterial {
    has_initialized: bool,
    color: Vector3<f32>,
    /// Peak opacity at the rim, 0..1.
    intensity: f32,
    /// Radius (in the units described on [`FRAGMENT_SHADER_SOURCE`]) where the
    /// tint starts, and where it reaches full [`intensity`](Self::intensity).
    inner_radius: f32,
    outer_radius: f32,
}

impl Material for VignetteMaterial {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    fn initialize(&mut self, is_opengl_es: bool) {
        let _ = SHADER_PROGRAM.get_or_init(|| {
            let vertex_shader = crate::shader::build(
                VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            let fragment_shader = crate::shader::build(
                FRAGMENT_SHADER_SOURCE,
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );

            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);
                let uniforms = Uniforms {
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    color_loc: gl::GetUniformLocation(shader.gl_id, c_str!("color").as_ptr()),
                    intensity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("intensity").as_ptr(),
                    ),
                    inner_radius_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("innerRadius").as_ptr(),
                    ),
                    outer_radius_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("outerRadius").as_ptr(),
                    ),
                };
                (shader, uniforms)
            }
        });

        self.has_initialized = true;
    }

    /// Always translucent - a vignette that wrote colour into the opaque pass
    /// would paint a solid rectangle over the world. The reported value is the
    /// transparency at the *rim*, which is what an observer (`/v1/scene`,
    /// a reviewer) means by "how strong is the effect".
    fn transparency(&self) -> Option<f32> {
        Some((1.0 - self.intensity).clamp(0.0, 1.0))
    }

    fn draw_opaque(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        false
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        unsafe {
            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform3fv(uniforms.color_loc, 1, self.color.as_ptr());
            gl::Uniform1f(uniforms.intensity_loc, self.intensity);
            gl::Uniform1f(uniforms.inner_radius_loc, self.inner_radius);
            gl::Uniform1f(uniforms.outer_radius_loc, self.outer_radius);
        }
        true
    }
}

pub fn create(
    color: Vector3<f32>,
    intensity: f32,
    inner_radius: f32,
    outer_radius: f32,
) -> Box<dyn Material> {
    Box::new(VignetteMaterial {
        has_initialized: false,
        color,
        intensity: intensity.clamp(0.0, 1.0),
        inner_radius,
        outer_radius,
    })
}
