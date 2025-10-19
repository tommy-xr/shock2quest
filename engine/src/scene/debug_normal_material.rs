extern crate gl;

use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::scene::light::Light;
use crate::shader_program::ShaderProgram;
use crate::texture::TextureTrait;
use c_string::*;
use cgmath::{Matrix, Matrix4};
use once_cell::sync::OnceCell;

// Debug material that visualizes normals as RGB colors for validation
const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 2) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec3 worldNormal;

        void main() {
            worldNormal = normalize(mat3(world) * inNormal);
            gl_Position = projection * view * world * vec4(inPos, 1.0);
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec3 worldNormal;

        void main() {
            // Convert normal from [-1,1] to [0,1] range for RGB visualization
            // Red=X, Green=Y, Blue=Z components of normal vectors
            vec3 normalColor = worldNormal * 0.5 + 0.5;
            fragColor = vec4(normalColor, 1.0);
        }
"#;

static mut SHADER: OnceCell<ShaderProgram> = OnceCell::new();

pub struct DebugNormalMaterial {
    texture: Box<dyn TextureTrait>,
    initialized: bool,
}

impl DebugNormalMaterial {
    pub fn create(texture: Box<dyn TextureTrait>) -> DebugNormalMaterial {
        DebugNormalMaterial { texture, initialized: false }
    }
}

impl Material for DebugNormalMaterial {
    fn has_initialized(&self) -> bool {
        self.initialized
    }

    fn initialize(&mut self, is_opengl_es: bool, _storage: &dyn crate::file_system::Storage) {
        unsafe {
            SHADER.get_or_init(|| {
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
                crate::shader_program::link(&vertex_shader, &fragment_shader)
            });
        }
        self.initialized = true;
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
    ) -> bool {
        let shader = unsafe { SHADER.get().expect("Shader should be initialized") };

        unsafe {
            gl::UseProgram(shader.gl_id);

            // Set uniforms
            let world_loc = gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr());
            let view_loc = gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr());
            let projection_loc = gl::GetUniformLocation(shader.gl_id, c_str!("projection").as_ptr());

            gl::UniformMatrix4fv(
                world_loc,
                1,
                gl::FALSE,
                world_matrix.as_ptr() as *const f32,
            );
            gl::UniformMatrix4fv(
                view_loc,
                1,
                gl::FALSE,
                view_matrix.as_ptr() as *const f32,
            );
            gl::UniformMatrix4fv(
                projection_loc,
                1,
                gl::FALSE,
                render_context.projection_matrix.as_ptr() as *const f32,
            );

            // Bind texture (even though we don't use it, keeps interface consistent)
            gl::ActiveTexture(gl::TEXTURE0);
            self.texture.bind0(render_context);
        }
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
    ) -> bool {
        // Normal debug materials are always opaque
        self.draw_opaque(render_context, view_matrix, world_matrix, skinning_data)
    }

    fn draw_light_pass(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _light: &dyn Light,
        _shadow_map: Option<&()>,
    ) -> bool {
        // Debug normal material doesn't participate in lighting passes
        false
    }
}

pub fn create(texture: Box<dyn TextureTrait>) -> Box<dyn Material> {
    Box::new(DebugNormalMaterial::create(texture))
}