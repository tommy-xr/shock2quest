extern crate gl;
use std::ops::Deref;

use crate::engine::EngineRenderContext;
use crate::scene::light::LightArray;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;
use crate::texture::TextureTrait;
use c_string::*;
use cgmath::{Matrix, Matrix4};
use once_cell::sync::OnceCell;

// Simple vertex shader - no normals needed for UI elements
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

// Simple fragment shader with horizontal clipping and color key
const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;

        uniform sampler2D texture1;
        uniform float clipX;  // 0.0 to 1.0 - anything past this X coord is clipped

        void main() {
            // Clip anything past the clipX threshold
            if (texCoord.x > clipX) {
                discard;
            }

            // Sample texture and output full-bright (emissive)
            vec4 texColor = texture(texture1, texCoord);
            if (texColor.a < 0.1) discard;  // Alpha test

            // Color key filtering: discard cyan-like colors (handle variations like r=0, g=246, b=255)
            // More forgiving tolerance for slight color variations in assets
            if (texColor.r < 0.02 && texColor.g > 0.9 && texColor.b > 0.95) {
                discard;
            }

            fragColor = texColor;  // Full-bright output
        }
"#;

struct ClippedScreenUniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    clip_x_loc: i32,
}

static CLIPPED_SCREEN_SHADER: OnceCell<(ShaderProgram, ClippedScreenUniforms)> = OnceCell::new();

pub struct ClippedScreenMaterial<T>
where
    T: Deref<Target = dyn TextureTrait + 'static>,
{
    has_initialized: bool,
    diffuse_texture: T,
    clip_percentage: f32, // 0.0 to 1.0
}

impl<T> ClippedScreenMaterial<T>
where
    T: Deref<Target = dyn TextureTrait>,
{
    pub fn set_clip_percentage(&mut self, percentage: f32) {
        self.clip_percentage = percentage.clamp(0.0, 1.0);
    }

    pub fn draw_unified(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
    ) {
        let (shader_program, uniforms) = CLIPPED_SCREEN_SHADER
            .get()
            .expect("clipped screen shader not compiled");

        self.diffuse_texture.bind0(render_context);

        unsafe {
            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;

            // Set transformation matrices
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());

            // Set clipping value
            gl::Uniform1f(uniforms.clip_x_loc, self.clip_percentage);
        }
    }
}

impl<T> Material for ClippedScreenMaterial<T>
where
    T: Deref<Target = dyn TextureTrait>,
{
    fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    fn initialize(&mut self, is_opengl_es: bool) {
        let _ = CLIPPED_SCREEN_SHADER.get_or_init(|| {
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

                let uniforms = ClippedScreenUniforms {
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    clip_x_loc: gl::GetUniformLocation(shader.gl_id, c_str!("clipX").as_ptr()),
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
        _lights: &LightArray,
    ) -> bool {
        self.draw_unified(render_context, view_matrix, world_matrix);
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &LightArray,
    ) -> bool {
        // Clipped screen materials can be rendered as transparent for proper blending
        self.draw_unified(render_context, view_matrix, world_matrix);
        true
    }
}

pub fn create<T>(diffuse_texture: T, clip_percentage: f32) -> Box<dyn Material>
where
    T: Deref<Target = dyn TextureTrait> + 'static,
{
    Box::new(ClippedScreenMaterial {
        diffuse_texture,
        has_initialized: false,
        clip_percentage: clip_percentage.clamp(0.0, 1.0),
    })
}
