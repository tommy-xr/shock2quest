extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use c_string::*;
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3};
use once_cell::sync::OnceCell;

const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform vec3 color;

        out vec3 vertexColor;

        void main() {
            vertexColor = color;
            gl_Position = projection * view * world * vec4(inPos, 1.0);
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec3 vertexColor;

        void main() {
            fragColor = vec4(vertexColor.rgb, 1.0);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    color_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct ColorMaterial {
    has_initialized: bool,
    pub color: Vector3<f32>,
}

impl Material for ColorMaterial {
    fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    fn initialize(&mut self, is_opengl_es: bool, _storage: &dyn crate::file_system::Storage) {
        let _ = SHADER_PROGRAM.get_or_init(|| {
            // build and compile our shader program
            // ------------------------------------
            // vertex shader
            let vertex_shader = crate::shader::build(
                VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            // fragment shader
            let fragment_shader = crate::shader::build(
                FRAGMENT_SHADER_SOURCE,
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );
            // link shaders

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
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        let p = shader_program;
        unsafe {
            gl::UseProgram(p.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform3fv(uniforms.color_loc, 1, self.color.as_ptr());
        }
        true
    }
}

pub fn create(color: Vector3<f32>) -> Box<dyn Material> {
    Box::new(ColorMaterial {
        has_initialized: false,
        color,
    })
}
