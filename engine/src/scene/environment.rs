//! The mission's captured environment cubemap: the direction mapping every
//! sampler shares, and a debug preview that paints it around the camera.
extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;
use crate::texture::CubeTexture;

use c_string::*;
use cgmath::Matrix4;
use cgmath::prelude::*;
use once_cell::sync::OnceCell;
use std::any::Any;
use std::rc::Rc;

/// Maps an engine direction into the 25AE capture's axes. Both are y-up; the
/// capture is turned a quarter about y and left-handed like a D3D/GL cube, so
/// text on the walls reads unmirrored. Matched against Earth's elevator doors,
/// which sit on engine +Z from the start and on the capture's -X face.
pub(crate) const GLSL: &str = r#"
vec3 environmentDirection(vec3 engineDirection) {
    return vec3(-engineDirection.z, engineDirection.y, -engineDirection.x);
}
"#;

const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec3 worldPos;

        void main() {
            vec4 position = world * vec4(inPos, 1.0);
            worldPos = position.xyz;
            gl_Position = projection * view * position;
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec3 worldPos;

        uniform samplerCube environment;
        uniform vec3 eye;

        void main() {
            vec3 direction = environmentDirection(normalize(worldPos - eye));
            fragColor = vec4(textureLod(environment, direction, 0.0).rgb, 1.0);
        }
"#;

struct Uniforms {
    world: i32,
    view: i32,
    projection: i32,
    environment: i32,
    eye: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

const ENVIRONMENT_UNIT: u32 = 0;

/// Draws the cubemap as seen from the eye, on whatever geometry surrounds it.
pub struct EnvironmentPreviewMaterial {
    has_initialized: bool,
    environment: Rc<CubeTexture>,
}

pub fn create_preview(environment: Rc<CubeTexture>) -> Box<dyn Material> {
    Box::new(EnvironmentPreviewMaterial {
        has_initialized: false,
        environment,
    })
}

impl Material for EnvironmentPreviewMaterial {
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
                &format!("{GLSL}\n{FRAGMENT_SHADER_SOURCE}"),
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );
            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);
                let loc =
                    |name: &std::ffi::CStr| gl::GetUniformLocation(shader.gl_id, name.as_ptr());
                let uniforms = Uniforms {
                    world: loc(c_str!("world")),
                    view: loc(c_str!("view")),
                    projection: loc(c_str!("projection")),
                    environment: loc(c_str!("environment")),
                    eye: loc(c_str!("eye")),
                };
                (shader, uniforms)
            }
        });
        self.has_initialized = true;
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        let (program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        let eye = crate::scene::material::eye_position(view_matrix);
        unsafe {
            gl::UseProgram(program.gl_id);
            gl::UniformMatrix4fv(uniforms.world, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(
                uniforms.projection,
                1,
                gl::FALSE,
                render_context.projection_matrix.as_ptr(),
            );
            gl::Uniform3fv(uniforms.eye, 1, eye.as_ptr());
            gl::Uniform1i(uniforms.environment, ENVIRONMENT_UNIT as i32);
            self.environment.bind_to(ENVIRONMENT_UNIT);
        }
        true
    }

    fn draw_transparent(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        false
    }
}
