//! Unlit "rift" surface: samples an offscreen pass where the pass's camera saw
//! the fragment, bent by the surface normal like a glass ball, with a fresnel rim.
extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::scene::render_target::{RenderTarget, TEXTURE_UNIT};
use crate::shader_program::ShaderProgram;

use c_string::*;
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3};
use once_cell::sync::OnceCell;
use std::any::Any;
use std::rc::Rc;

const VERTEX_SHADER_SOURCE: &str = r#"
        #ifdef GL_ES
        precision highp float;
        #endif
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inUv;
        layout (location = 2) in vec3 inNormal;
        out vec4 sampleClip;
        out vec3 viewPos;
        out vec3 viewNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform mat4 sampleViewProjection;

        void main() {
            vec4 worldPos = world * vec4(inPos, 1.0);
            vec4 eye = view * worldPos;
            viewPos = eye.xyz;
            viewNormal = mat3(view * world) * inNormal;
            // Where the pass saw this point, not this eye's clip position:
            // one pass can serve both VR eyes.
            sampleClip = sampleViewProjection * worldPos;
            gl_Position = projection * eye;
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        #ifdef GL_ES
        precision highp float;
        #endif
        out vec4 fragColor;

        in vec4 sampleClip;
        in vec3 viewPos;
        in vec3 viewNormal;

        uniform sampler2D portalTexture;
        // The NDC rect (min.xy, max.xy) of the sampling view the texture covers.
        uniform vec4 sampleRect;
        uniform vec3 rimColor;
        // x: refraction strength, y: rim exponent, z: time (s), w: swirl
        // strength. Strengths are in screen-UV units of the sampling view.
        uniform vec4 params;

        void main() {
            vec3 n = normalize(viewNormal);
            float facing = abs(dot(n, normalize(-viewPos)));
            vec2 ndc = sampleClip.xy / sampleClip.w;
            ndc += n.xy * params.x * 2.0;
            // A slow swirl, strongest toward the rim, so it reads as a rift
            // rather than a lens.
            float edge = 1.0 - facing;
            float angle = params.z * 0.8 + edge * 6.0;
            ndc += edge * params.w * 2.0 * vec2(sin(angle + n.y * 5.0), cos(angle * 1.3 + n.x * 5.0));
            vec2 uv = (ndc - sampleRect.xy) / (sampleRect.zw - sampleRect.xy);
            // Cool the far side slightly so the window never matches its backdrop.
            vec3 view = texture(portalTexture, clamp(uv, 0.0, 1.0)).rgb * vec3(0.8, 1.0, 1.2);
            float rim = pow(edge, params.y);
            float pulse = 0.85 + 0.15 * sin(params.z * 2.5);
            vec3 color = mix(view, rimColor, clamp(rim * 1.2, 0.0, 1.0)) + rimColor * rim * pulse * 0.6;
            fragColor = vec4(color, 1.0);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    sample_view_projection_loc: i32,
    sample_rect_loc: i32,
    texture_loc: i32,
    rim_color_loc: i32,
    params_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

/// Rim swirl strength, in screen-UV units like `distortion`.
const SWIRL: f32 = 0.03;

/// How far (NDC units) past the sphere's silhouette `distortion` and the swirl
/// can push a sample: the margin the pass must render around the sphere.
pub fn sample_margin(distortion: f32) -> f32 {
    2.0 * (distortion + SWIRL)
}

pub struct PortalMaterial {
    has_initialized: bool,
    target: Rc<RenderTarget>,
    rim_color: Vector3<f32>,
    /// How far the surface normal pushes the sample, in screen-UV units.
    distortion: f32,
    /// Fresnel exponent: higher keeps the rim thinner.
    rim_power: f32,
}

impl Material for PortalMaterial {
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
                let id = shader.gl_id;
                let uniforms = Uniforms {
                    world_loc: gl::GetUniformLocation(id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(id, c_str!("projection").as_ptr()),
                    sample_view_projection_loc: gl::GetUniformLocation(
                        id,
                        c_str!("sampleViewProjection").as_ptr(),
                    ),
                    sample_rect_loc: gl::GetUniformLocation(id, c_str!("sampleRect").as_ptr()),
                    texture_loc: gl::GetUniformLocation(id, c_str!("portalTexture").as_ptr()),
                    rim_color_loc: gl::GetUniformLocation(id, c_str!("rimColor").as_ptr()),
                    params_loc: gl::GetUniformLocation(id, c_str!("params").as_ptr()),
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
        let texture = self.target.color_texture();
        let Some(sampling) = self.target.sampling().filter(|_| texture != 0) else {
            return false;
        };
        let (program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        unsafe {
            gl::UseProgram(program.gl_id);
            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::UniformMatrix4fv(
                uniforms.sample_view_projection_loc,
                1,
                gl::FALSE,
                sampling.view_projection.as_ptr(),
            );
            let rect = sampling.rect;
            gl::Uniform4f(
                uniforms.sample_rect_loc,
                rect.min.x,
                rect.min.y,
                rect.max.x,
                rect.max.y,
            );
            gl::Uniform3fv(uniforms.rim_color_loc, 1, self.rim_color.as_ptr());
            gl::Uniform4f(
                uniforms.params_loc,
                self.distortion,
                self.rim_power,
                render_context.time,
                SWIRL,
            );
            gl::ActiveTexture(gl::TEXTURE0 + TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_2D, texture);
            gl::Uniform1i(uniforms.texture_loc, TEXTURE_UNIT as i32);
            gl::ActiveTexture(gl::TEXTURE0);
        }
        true
    }
}

pub fn create(
    target: Rc<RenderTarget>,
    rim_color: Vector3<f32>,
    distortion: f32,
    rim_power: f32,
) -> Box<dyn Material> {
    Box::new(PortalMaterial {
        has_initialized: false,
        target,
        rim_color,
        distortion,
        rim_power,
    })
}
