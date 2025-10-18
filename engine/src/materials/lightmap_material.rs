extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;
use crate::texture::Texture;
use crate::texture::TextureTrait;
use c_string::*;
use cgmath::prelude::*;

use cgmath::Matrix4;
use once_cell::sync::OnceCell;
use std::rc::Rc;

const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;
        layout (location = 2) in vec2 inLightMapTex;
        layout (location = 3) in vec4 inAtlas;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        out vec2 texCoord;
        out highp vec2 lightMapTexCoord;
        out highp vec4 atlasCoord;

        void main() {
            texCoord = inTex;
            lightMapTexCoord = inLightMapTex;
            atlasCoord = inAtlas;
            gl_Position = projection * view * world * vec4(inPos, 1.0);
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in highp vec2 lightMapTexCoord;
        in highp vec4 atlasCoord;

        uniform vec4 atlas;
        // texture sampler
        uniform sampler2D texture1;
        uniform sampler2D texture2;

        void main() {

            float half_pixel = 0.5 / 4096.0;
            float full_pixel = half_pixel * 2.0;
            vec2 wrappedTexCoord = vec2(0.0, 0.0);
            float width = atlasCoord.z - full_pixel;
            float height = atlasCoord.w - full_pixel;

            wrappedTexCoord.x = mod(lightMapTexCoord.x * width, width) + atlasCoord.x + half_pixel;
            wrappedTexCoord.y = mod(lightMapTexCoord.y * height, height) + atlasCoord.y + half_pixel;

            vec4 lightmapColor = texture(texture1, wrappedTexCoord);
            vec4 diffuseColor = texture(texture2, texCoord);
            float attn_factor = 1.0;
            vec4 attenuation = vec4(attn_factor, attn_factor, attn_factor, 1.0);
            fragColor = diffuseColor * lightmapColor * attn_factor;
            fragColor.a = 1.0;

        }
"#;

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    texture1_loc: i32,
    texture2_loc: i32,
}

pub struct LightmapMaterial {
    has_initialized: bool,
    lightmap_texture: Rc<Texture>,
    diffuse_texture: Rc<dyn TextureTrait>,
}

impl LightmapMaterial {
    pub fn create(
        lightmap_texture: Rc<Texture>,
        diffuse_texture: Rc<dyn TextureTrait>,
    ) -> Box<dyn Material> {
        Box::new(LightmapMaterial {
            diffuse_texture,
            lightmap_texture,
            has_initialized: false,
        })
    }
}

impl Material for LightmapMaterial {
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
                    texture1_loc: gl::GetUniformLocation(shader.gl_id, c_str!("texture1").as_ptr()),
                    texture2_loc: gl::GetUniformLocation(shader.gl_id, c_str!("texture2").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
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
    ) -> bool {
        unsafe {
            let (p, uniforms) = SHADER_PROGRAM.get().unwrap();
            crate::texture::bind0(&self.lightmap_texture);
            self.diffuse_texture.bind1(render_context);
            //crate::texture::bind1(&self.diffuse_texture);

            gl::UseProgram(p.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform1i(uniforms.texture1_loc, 0);
            gl::Uniform1i(uniforms.texture2_loc, 1);
        }
        true
    }
}
