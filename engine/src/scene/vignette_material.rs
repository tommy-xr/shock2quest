extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use c_string::*;
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3};
use once_cell::sync::OnceCell;
use std::any::Any;

/// A radial comfort mask. The default draws a tinted rim on a view-locked
/// quad. `create_grid` uses the same soft central opening on a stage-fixed
/// cube, drawing only thin antialiased grid lines around the periphery.
const VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec2 texCoord;
        out vec3 viewPosition;

        void main() {
            texCoord = inTex;
            vec4 position = view * world * vec4(inPos, 1.0);
            viewPosition = position.xyz;
            gl_Position = projection * position;
        }
"#;

/// `radius` is 0 at the quad's centre and 1 at the middle of each edge (so it
/// keeps going past 1 into the corners, which stay fully tinted).
const FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in vec3 viewPosition;
        uniform mat4 projection;
        uniform float gridCells;
        uniform float wavePhase;

        uniform vec3 color;
        uniform float intensity;
        uniform float innerRadius;
        uniform float outerRadius;

        void main() {
            float radius = length(texCoord - vec2(0.5, 0.5)) * 2.0;
            float pattern = 1.0;
            if (gridCells > 0.0) {
                // Cube UVs keep the pattern fixed to the stage. Only the
                // aperture follows the eye, retaining a clear aiming area.
                vec2 cell = texCoord * gridCells;
                vec2 distanceToLine = abs(fract(cell + 0.5) - 0.5);
                vec2 pixels = distanceToLine / max(fwidth(cell), vec2(0.00001));
                vec2 lines = 1.0 - smoothstep(vec2(0.35), vec2(1.35), pixels);
                pattern = max(lines.x, lines.y);
                vec2 tangent = viewPosition.xy / max(-viewPosition.z, 0.0001);
                radius = length(tangent * vec2(projection[0][0], projection[1][1]));
            }
            vec3 tint = color;
            if (wavePhase >= 0.0) {
                // Distort the translucent pattern's UVs, not the tracked view.
                // The aperture uses the original UV radius so waves never
                // cross the clear aiming area. Both eyes share one sim phase.
                vec2 uv = texCoord * 2.0 - 1.0;
                uv += 0.06 * vec2(sin(uv.y * 5.0 + wavePhase),
                                 cos(uv.x * 4.0 - wavePhase));
                float wave = 0.5 + 0.5 * sin(uv.y * 12.0 + uv.x * 4.0
                                           + 1.5 * sin(uv.x * 5.0 - wavePhase)
                                           + wavePhase);
                pattern = 0.25 + 0.75 * wave * wave;
                tint = mix(color, vec3(0.25, 0.48, 0.58), wave * 0.4);
            }
            float span = max(outerRadius - innerRadius, 0.0001);
            float t = clamp((radius - innerRadius) / span, 0.0, 1.0);
            // Smoothstep, so the rim has no visible banding edge where it
            // meets the clear centre.
            float ramp = t * t * (3.0 - 2.0 * t);
            fragColor = vec4(tint, pattern * ramp * intensity);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    color_loc: i32,
    intensity_loc: i32,
    inner_radius_loc: i32,
    outer_radius_loc: i32,
    grid_cells_loc: i32,
    wave_phase_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct VignetteMaterial {
    has_initialized: bool,
    color: Vector3<f32>,
    /// Peak opacity at the rim, 0..1.
    intensity: f32,
    /// Radius (in the units described on [`FRAGMENT_SHADER_SOURCE`]) where the
    /// tint starts, and where it reaches full [`intensity`](Self::intensity).
    inner_radius: f32,
    outer_radius: f32,
    grid_cells: f32,
    /// Negative disables waves for all existing damage and comfort masks.
    wave_phase: f32,
}

impl VignetteMaterial {
    /// Animate only the translucent pattern. Does not modify projection or depth.
    pub fn set_wave_phase(&mut self, phase: f32) {
        self.wave_phase = phase;
    }
}

impl Material for VignetteMaterial {
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
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    color_loc: gl::GetUniformLocation(shader.gl_id, c_str!("color").as_ptr()),
                    intensity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("intensity").as_ptr(),
                    ),
                    inner_radius_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("innerRadius").as_ptr(),
                    ),
                    wave_phase_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("wavePhase").as_ptr(),
                    ),
                    grid_cells_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("gridCells").as_ptr(),
                    ),
                    outer_radius_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("outerRadius").as_ptr(),
                    ),
                };
                (shader, uniforms)
            }
        });

        self.has_initialized = true;
    }

    /// Always translucent - a vignette that wrote colour into the opaque pass
    /// would paint a solid rectangle over the world. The reported value is the
    /// transparency at the *rim*, which is what an observer (`/v1/scene`,
    /// a reviewer) means by "how strong is the effect".
    fn transparency(&self) -> Option<f32> {
        Some((1.0 - self.intensity).clamp(0.0, 1.0))
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
        let (shader_program, uniforms) = SHADER_PROGRAM.get().expect("shader not compiled");
        unsafe {
            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform3fv(uniforms.color_loc, 1, self.color.as_ptr());
            gl::Uniform1f(uniforms.intensity_loc, self.intensity);
            gl::Uniform1f(uniforms.inner_radius_loc, self.inner_radius);
            gl::Uniform1f(uniforms.outer_radius_loc, self.outer_radius);
            gl::Uniform1f(uniforms.grid_cells_loc, self.grid_cells);
            gl::Uniform1f(uniforms.wave_phase_loc, self.wave_phase);
        }
        true
    }
}

pub fn create(
    color: Vector3<f32>,
    intensity: f32,
    inner_radius: f32,
    outer_radius: f32,
) -> Box<dyn Material> {
    Box::new(VignetteMaterial {
        has_initialized: false,
        color,
        intensity: intensity.clamp(0.0, 1.0),
        inner_radius,
        outer_radius,
        grid_cells: 0.0,
        wave_phase: -1.0,
    })
}

/// Thin procedural cage lines with a soft, gaze-centered clear opening.
/// `cells` is the number of equal grid intervals along each cube face.
pub fn create_grid(color: Vector3<f32>, intensity: f32, cells: f32) -> Box<dyn Material> {
    Box::new(VignetteMaterial {
        has_initialized: false,
        color,
        intensity: intensity.clamp(0.0, 1.0),
        inner_radius: 0.5,
        outer_radius: 0.85,
        grid_cells: cells.max(1.0),
        wave_phase: -1.0,
    })
}
