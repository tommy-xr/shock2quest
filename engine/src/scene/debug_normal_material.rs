extern crate gl;

use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;
use c_string::*;
use cgmath::{Matrix, Matrix4};
use once_cell::sync::OnceCell;
use std::ffi::CString;

// Debug material that visualizes normals as RGB colors for validation
const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 2) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec3 worldNormal;

        void main() {
            mat3 normalMatrix = transpose(inverse(mat3(world)));
            worldNormal = normalize(normalMatrix * inNormal);
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

static SHADER: OnceCell<ShaderProgram> = OnceCell::new();
static SKINNED_SHADER: OnceCell<ShaderProgram> = OnceCell::new();

pub struct DebugNormalMaterial {
    initialized: bool,
}

impl DebugNormalMaterial {
    pub fn new() -> DebugNormalMaterial {
        DebugNormalMaterial { initialized: false }
    }
}

impl Material for DebugNormalMaterial {
    fn has_initialized(&self) -> bool {
        self.initialized
    }

    fn initialize(&mut self, is_opengl_es: bool, _storage: &dyn crate::file_system::Storage) {
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
        self.initialized = true;
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        let shader = SHADER.get().expect("Shader should be initialized");

        unsafe {
            gl::UseProgram(shader.gl_id);

            // Set uniforms
            let world_loc = gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr());
            let view_loc = gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr());
            let projection_loc =
                gl::GetUniformLocation(shader.gl_id, c_str!("projection").as_ptr());

            gl::UniformMatrix4fv(world_loc, 1, gl::FALSE, world_matrix.as_ptr() as *const f32);
            gl::UniformMatrix4fv(view_loc, 1, gl::FALSE, view_matrix.as_ptr() as *const f32);
            gl::UniformMatrix4fv(
                projection_loc,
                1,
                gl::FALSE,
                render_context.projection_matrix.as_ptr() as *const f32,
            );

            // Bind texture (even though we don't use it, keeps interface consistent)
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        // Normal debug materials are always opaque
        self.draw_opaque(
            render_context,
            view_matrix,
            world_matrix,
            skinning_data,
            lights,
        )
    }
}

pub fn create() -> Box<dyn Material> {
    Box::new(DebugNormalMaterial::new())
}

const SKINNED_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;
        layout (location = 2) in ivec4 bone_ids;
        layout (location = 3) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform mat4 bone_matrices[40];

        out vec3 worldNormal;

        void main() {
            vec4 skinnedPos = bone_matrices[bone_ids.x] * vec4(inPos, 1.0);
            vec3 skinnedNormal = mat3(bone_matrices[bone_ids.x]) * inNormal;
            vec4 worldPosition = world * skinnedPos;
            worldNormal = normalize(mat3(world) * skinnedNormal);
            gl_Position = projection * view * worldPosition;
        }
"#;

const SKINNED_FRAGMENT_SHADER_SOURCE: &str = FRAGMENT_SHADER_SOURCE;

pub struct DebugNormalSkinnedMaterial {
    initialized: bool,
}

impl DebugNormalSkinnedMaterial {
    pub fn new() -> DebugNormalSkinnedMaterial {
        DebugNormalSkinnedMaterial { initialized: false }
    }
}

impl Material for DebugNormalSkinnedMaterial {
    fn has_initialized(&self) -> bool {
        self.initialized
    }

    fn initialize(&mut self, is_opengl_es: bool, _storage: &dyn crate::file_system::Storage) {
        SKINNED_SHADER.get_or_init(|| {
            let vertex_shader = crate::shader::build(
                SKINNED_VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );
            let fragment_shader = crate::shader::build(
                SKINNED_FRAGMENT_SHADER_SOURCE,
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );
            crate::shader_program::link(&vertex_shader, &fragment_shader)
        });
        self.initialized = true;
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        let shader = SKINNED_SHADER.get().expect("Shader should be initialized");

        unsafe {
            gl::UseProgram(shader.gl_id);

            let world_loc = gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr());
            let view_loc = gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr());
            let projection_loc =
                gl::GetUniformLocation(shader.gl_id, c_str!("projection").as_ptr());

            gl::UniformMatrix4fv(world_loc, 1, gl::FALSE, world_matrix.as_ptr() as *const f32);
            gl::UniformMatrix4fv(view_loc, 1, gl::FALSE, view_matrix.as_ptr() as *const f32);
            gl::UniformMatrix4fv(
                projection_loc,
                1,
                gl::FALSE,
                render_context.projection_matrix.as_ptr() as *const f32,
            );

            for i in 0..skinning_data.len() {
                let name = format!("bone_matrices[{i}]");
                let c_str = CString::new(name).unwrap();
                let loc = gl::GetUniformLocation(shader.gl_id, c_str.as_ptr());
                gl::UniformMatrix4fv(loc, 1, gl::FALSE, skinning_data[i].as_ptr());
            }

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        self.draw_opaque(
            render_context,
            view_matrix,
            world_matrix,
            skinning_data,
            lights,
        )
    }
}

pub fn create_skinned() -> Box<dyn Material> {
    Box::new(DebugNormalSkinnedMaterial::new())
}
