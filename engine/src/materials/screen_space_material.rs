extern crate gl;
use std::any::Any;
use std::rc::Rc;

use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use crate::texture::TextureTrait;
use c_string::*;
use cgmath::prelude::*;
use cgmath::Matrix4;
use cgmath::Vector4;

use once_cell::sync::OnceCell;

const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec2 inPos;
        layout (location = 1) in vec2 inTex;

        uniform mat4 world;
        uniform mat4 projection;
        out vec2 texCoord;

        void main() {
            texCoord = inTex;
            gl_Position = projection * world * vec4(inPos.xy, 0.0, 1.0);
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;

        uniform vec4 textColor;
        uniform sampler2D fontTexture;

        void main()
        {    
            vec4 sampled = texture(fontTexture, texCoord);
            fragColor = textColor * sampled;
        }  
"#;

struct Uniforms {
    projection_loc: i32,
    world_loc: i32,
    text_color_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct ScreenSpaceMaterial {
    diffuse_texture: Rc<dyn TextureTrait>,
    text_color: Vector4<f32>,
}

impl ScreenSpaceMaterial {
    pub fn create(
        diffuse_texture: Rc<dyn TextureTrait>,
        text_color: Vector4<f32>,
    ) -> Box<dyn Material> {
        Box::new(ScreenSpaceMaterial {
            diffuse_texture,
            text_color,
        })
    }
    pub fn draw_common(
        &self,
        render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
    ) {
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        self.diffuse_texture.bind0(render_context);
        unsafe {
            gl::UseProgram(shader_program.gl_id);

            // TODO: Pass in via render_context?
            // Separate render pass?
            let projection = cgmath::ortho(
                0.0,
                render_context.screen_size.x,
                render_context.screen_size.y,
                0.0,
                0.0,
                1.0,
            );

            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform4f(
                uniforms.text_color_loc,
                self.text_color.x,
                self.text_color.y,
                self.text_color.z,
                self.text_color.w,
            );
        }
    }
}
impl Material for ScreenSpaceMaterial {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn has_initialized(&self) -> bool {
        false
    }

    fn initialize(&mut self, is_opengl_es: bool) {
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
                    text_color_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("textColor").as_ptr(),
                    ),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                };
                (shader, uniforms)
            }
        });
    }

    fn draw_opaque(
        &self,
        _render_context: &EngineRenderContext,
        _view_matrix: &Matrix4<f32>,
        _world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        // no-op
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
        self.draw_common(render_context, view_matrix, world_matrix);
        true
    }
}
