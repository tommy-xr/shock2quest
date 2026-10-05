//! Unlit laser spot and cylindrical core/halo. No scene-wide fog or trails.
extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use c_string::*;
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3};
use once_cell::sync::OnceCell;
use std::any::Any;

const VERTEX_SHADER_SOURCE: &str = r#"
        #ifdef GL_ES
        precision highp float;
        #endif
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inUv;
        layout (location = 2) in vec3 inNormal;
        out vec2 uv;
        out vec3 viewPos;
        out vec3 viewNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform vec3 color;

        out vec3 vertexColor;

        void main() {
            uv = inUv;
            viewPos = (view * world * vec4(inPos, 1.0)).xyz;
            viewNormal = mat3(view * world) * inNormal;
            vertexColor = color;
            gl_Position = projection * view * world * vec4(inPos, 1.0);
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        #ifdef GL_ES
        precision highp float;
        #endif
        out vec4 fragColor;

        in vec3 vertexColor;
        in vec2 uv;
        in vec3 viewPos;
        in vec3 viewNormal;
        // x: dot=0, halo=1, core=2. y: simulation time, not wall clock.
        uniform vec2 beam;

        uniform float transparency;

        void main() {
            if (beam.x > 0.5) {
                float facing = abs(dot(normalize(viewNormal), normalize(-viewPos)));
                float ends = smoothstep(0.0, 0.015, uv.y) * (1.0 - smoothstep(0.8, 1.0, uv.y));
                float smoke = 0.65 + 0.35 * sin(uv.y * 87.0 - beam.y * 1.4 + sin(uv.x * 18.84955592 + beam.y))
                                          * sin(uv.y * 31.0 + uv.x * 12.56637061 - beam.y * 0.6);
                float alpha = beam.x > 1.5 ? 0.8 * pow(facing, 0.7) : 0.24 * pow(facing, 1.8) * smoke;
                fragColor = vec4(vertexColor, alpha * ends * (1.0 - transparency));
                return;
            }
            float radius = length(uv * 2.0 - 1.0);
            float halo = pow(max(1.0 - radius, 0.0), 2.0);
            float core = 1.0 - smoothstep(0.12, 0.3, radius);
            float alpha = max(core, halo * 0.7) * (1.0 - transparency);
            if (alpha < 0.005) discard;
            fragColor = vec4(mix(vertexColor, vec3(1.0, 0.7, 0.6), core * 0.5), alpha);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    color_loc: i32,
    transparency_loc: i32,
    beam_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct LaserMaterial {
    has_initialized: bool,
    pub color: Vector3<f32>,
    /// Additional opacity override, applied on top of the radial alpha.
    transparency: f32,
    beam: [f32; 2],
}

impl Material for LaserMaterial {
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
                    beam_loc: gl::GetUniformLocation(shader.gl_id, c_str!("beam").as_ptr()),
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    color_loc: gl::GetUniformLocation(shader.gl_id, c_str!("color").as_ptr()),
                    transparency_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("transparency").as_ptr(),
                    ),
                };
                (shader, uniforms)
            }
        });

        self.has_initialized = true;
    }

    fn set_transparency_override(&mut self, transparency: Option<f32>) {
        self.transparency = transparency.map_or(0.0, |value| value.clamp(0.0, 1.0));
    }

    fn transparency(&self) -> Option<f32> {
        Some(self.transparency)
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
        self.draw(render_context, view_matrix, world_matrix);
        true
    }
}

impl LaserMaterial {
    fn draw(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
    ) {
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        let p = shader_program;
        unsafe {
            gl::UseProgram(p.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform3fv(uniforms.color_loc, 1, self.color.as_ptr());
            gl::Uniform1f(uniforms.transparency_loc, self.transparency);
            gl::Uniform2f(uniforms.beam_loc, self.beam[0], self.beam[1]);
        }
    }
}

pub fn create(color: Vector3<f32>) -> Box<dyn Material> {
    Box::new(LaserMaterial {
        has_initialized: false,
        color,
        transparency: 0.0,
        beam: [0.0, 0.0],
    })
}

/// The same shader shades the two cached cylinder instances for each sight.
pub fn create_beam(core: bool, seconds: f32) -> Box<dyn Material> {
    Box::new(LaserMaterial {
        has_initialized: false,
        color: if core {
            Vector3::new(1.0, 0.08, 0.025)
        } else {
            Vector3::new(1.0, 0.01, 0.005)
        },
        transparency: 0.0,
        beam: [if core { 2.0 } else { 1.0 }, seconds],
    })
}
