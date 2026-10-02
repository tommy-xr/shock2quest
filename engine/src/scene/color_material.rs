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

        uniform float transparency;

        void main() {
            fragColor = vec4(vertexColor.rgb, 1.0 - transparency);
        }
"#;

/// The shaded-grid variant: flat face shading from a fixed directional light
/// (tops bright, sides mid, undersides dark) plus 1-unit world-space grid
/// lines, so untextured debug geometry reads for depth and scale. The face
/// normal comes from screen-space derivatives because the cube carries none.
const GRID_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec3 worldPos;

        void main() {
            vec4 p = world * vec4(inPos, 1.0);
            worldPos = p.xyz;
            gl_Position = projection * view * p;
        }
"#;

const GRID_FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in highp vec3 worldPos;

        uniform vec3 color;
        uniform float transparency;

        void main() {
            vec3 n = normalize(cross(dFdx(worldPos), dFdy(worldPos)));
            // Light from above, slightly off-axis so the two side axes differ.
            vec3 light_dir = normalize(vec3(0.35, 1.0, 0.55));
            float shade = 0.45 + 0.55 * max(dot(n, light_dir), 0.0)
                - 0.2 * max(-n.y, 0.0);
            // Distance to the nearest 1-unit line, per in-plane axis.
            vec3 cell = abs(fract(worldPos - 0.5) - 0.5) / max(fwidth(worldPos), vec3(1e-4));
            vec3 in_plane = 1.0 - abs(n);
            float line = 1.0;
            if (in_plane.x > 0.5) line = min(line, cell.x);
            if (in_plane.y > 0.5) line = min(line, cell.y);
            if (in_plane.z > 0.5) line = min(line, cell.z);
            float grid = mix(0.72, 1.0, clamp(line, 0.0, 1.0));
            fragColor = vec4(color * shade * grid, 1.0 - transparency);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    color_loc: i32,
    transparency_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();
static GRID_SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();

pub struct ColorMaterial {
    has_initialized: bool,
    pub color: Vector3<f32>,
    /// 0.0 = opaque, 1.0 = invisible. Authored opaque; a translucent draw
    /// comes from a per-object `SceneObject::set_transparency` override.
    transparency: f32,
    /// Draw with the shaded-grid shader instead of flat colour.
    shaded_grid: bool,
}

impl ColorMaterial {
    fn is_transparent(&self) -> bool {
        self.transparency > 0.01
    }
}

impl Material for ColorMaterial {
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
        let (program, vertex, fragment) = if self.shaded_grid {
            (
                &GRID_SHADER_PROGRAM,
                GRID_VERTEX_SHADER_SOURCE,
                GRID_FRAGMENT_SHADER_SOURCE,
            )
        } else {
            (
                &SHADER_PROGRAM,
                VERTEX_SHADER_SOURCE,
                FRAGMENT_SHADER_SOURCE,
            )
        };
        let _ = program.get_or_init(|| {
            let vertex_shader =
                crate::shader::build(vertex, crate::shader::ShaderType::Vertex, is_opengl_es);
            let fragment_shader =
                crate::shader::build(fragment, crate::shader::ShaderType::Fragment, is_opengl_es);

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

    /// Opaque reports `None`, as it did before this material could blend at
    /// all: every procedural object in the game uses this material, and
    /// flipping them all from `null` to `0.0` in `/v1/scene` would change an
    /// observable API for no gain. A translucent draw comes from a per-object
    /// override, which the caller reports itself.
    fn transparency(&self) -> Option<f32> {
        self.is_transparent().then_some(self.transparency)
    }

    fn draw_opaque(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        if self.is_transparent() {
            return false;
        }
        self.draw(render_context, view_matrix, world_matrix);
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        if !self.is_transparent() {
            return false;
        }
        self.draw(render_context, view_matrix, world_matrix);
        true
    }
}

impl ColorMaterial {
    fn draw(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
    ) {
        let program = if self.shaded_grid {
            &GRID_SHADER_PROGRAM
        } else {
            &SHADER_PROGRAM
        };
        let (shader_program, uniforms) = program.get().expect("shader not compiled");
        let p = shader_program;
        unsafe {
            gl::UseProgram(p.gl_id);

            let projection = render_context.projection_matrix;
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            gl::Uniform3fv(uniforms.color_loc, 1, self.color.as_ptr());
            gl::Uniform1f(uniforms.transparency_loc, self.transparency);
        }
    }
}

pub fn create(color: Vector3<f32>) -> Box<dyn Material> {
    Box::new(ColorMaterial {
        has_initialized: false,
        color,
        transparency: 0.0,
        shaded_grid: false,
    })
}

/// `color`, shaded by face direction with a 1-unit world grid - for debug
/// scene geometry that would otherwise blend into one flat silhouette.
pub fn create_shaded_grid(color: Vector3<f32>) -> Box<dyn Material> {
    Box::new(ColorMaterial {
        has_initialized: false,
        color,
        transparency: 0.0,
        shaded_grid: true,
    })
}
