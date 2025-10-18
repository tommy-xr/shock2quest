extern crate gl;
use std::ops::Deref;

use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use crate::texture::TextureTrait;
use c_string::*;
use cgmath::prelude::*;
use cgmath::Matrix4;

use once_cell::sync::OnceCell;

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

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;

        uniform vec3 inColor;
        // texture sampler
        uniform sampler2D texture1;

        uniform float emissivity;
        uniform float transparency;

        void main() {

            // TODO: Revert
            //fragColor = vec4(texCoord.xy, 0.0, 1.0);
            vec4 texColor = texture(texture1, texCoord);
            if (texColor.a < 0.1) discard;
            fragColor = texColor * vec4(0.5, 0.5, 0.5, 1.0);
            fragColor.rgb += texColor.rgb * emissivity;
            fragColor.a *= 1.0 - transparency;
            //fragColor = vec4(vertexColor.rgb, 1.0);

        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    emissivity_loc: i32,
    transparency_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait + 'static>,
{
    has_initialized: bool,
    diffuse_texture: T,
    emissivity: f32,
    transparency: f32,
}

impl<T> BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait>,
{
    pub fn is_transparent(&self) -> bool {
        self.transparency > 0.01
    }

    pub fn draw_common(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
    ) {
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        self.diffuse_texture.bind0(render_context);
        unsafe {
            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;

            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform1f(uniforms.transparency_loc, self.transparency);
            gl::Uniform1f(uniforms.emissivity_loc, self.emissivity);
        }
    }
}
impl<T> Material for BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait>,
{
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
                    emissivity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("emissivity").as_ptr(),
                    ),
                    transparency_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("transparency").as_ptr(),
                    ),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                };
                (shader, uniforms)
            }
        });

        // self.diffuse_texture_descriptor
        //     .borrow_mut()
        //     .initialize(storage);
        self.has_initialized = true;
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
    ) -> bool {
        if !self.is_transparent() {
            self.draw_common(render_context, view_matrix, world_matrix);
            true
        } else {
            false
        }
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
    ) -> bool {
        if self.is_transparent() {
            self.draw_common(render_context, view_matrix, world_matrix);
            true
        } else {
            false
        }
    }
}

pub fn create<T>(diffuse_texture: T, emissivity: f32, transparency: f32) -> Box<dyn Material>
where
    T: Deref<Target = dyn TextureTrait> + 'static,
{
    Box::new(BasicMaterial {
        diffuse_texture,
        has_initialized: false,
        emissivity,
        transparency,
    })
}
