extern crate gl;
use std::ops::Deref;

use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::scene::light::{Light, LightType};
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

// Lighting pass shaders for multi-pass lighting system
const LIGHTING_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec2 texCoord;
        out vec3 worldPos;

        void main() {
            texCoord = inTex;
            vec4 worldPosition = world * vec4(inPos, 1.0);
            worldPos = worldPosition.xyz;
            gl_Position = projection * view * worldPosition;
        }
"#;

const LIGHTING_FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in vec3 worldPos;

        // texture sampler
        uniform sampler2D texture1;

        // Light parameters
        uniform vec3 lightPos;
        uniform vec4 lightColorIntensity;
        uniform vec3 lightDirection;
        uniform float lightInnerConeAngle;
        uniform float lightOuterConeAngle;
        uniform float lightRange;

        void main() {
            vec4 texColor = texture(texture1, texCoord);
            if (texColor.a < 0.1) discard;

            // Calculate lighting
            vec3 lightVec = lightPos - worldPos;
            float distance = length(lightVec);

            // Range check
            if (distance > lightRange) {
                discard;
            }

            vec3 lightDir = normalize(lightVec);

            // Cone attenuation for spotlight
            float cosOuterCone = cos(lightOuterConeAngle);
            float cosInnerCone = cos(lightInnerConeAngle);
            float spotFactor = dot(-lightDir, normalize(lightDirection));

            if (spotFactor < cosOuterCone) {
                discard;
            }

            float coneAttenuation = 1.0;
            if (spotFactor < cosInnerCone) {
                coneAttenuation = (spotFactor - cosOuterCone) / (cosInnerCone - cosOuterCone);
            }

            // Distance attenuation
            float distanceAttenuation = 1.0 / (1.0 + 0.1 * distance + 0.01 * distance * distance);

            // Simple diffuse lighting (assume normal pointing up for now)
            vec3 normal = vec3(0.0, 1.0, 0.0);
            float lambertian = max(dot(normal, lightDir), 0.0);

            // Combine all factors
            vec3 lightContribution = texColor.rgb * lightColorIntensity.rgb * lightColorIntensity.a
                                   * lambertian * coneAttenuation * distanceAttenuation;

            fragColor = vec4(lightContribution, texColor.a);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    emissivity_loc: i32,
    transparency_loc: i32,
}

struct LightingUniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    light_pos_loc: i32,
    light_color_intensity_loc: i32,
    light_direction_loc: i32,
    light_inner_cone_angle_loc: i32,
    light_outer_cone_angle_loc: i32,
    light_range_loc: i32,
}

static SHADER_PROGRAM: OnceCell<(ShaderProgram, Uniforms)> = OnceCell::new();
static LIGHTING_SHADER_PROGRAM: OnceCell<(ShaderProgram, LightingUniforms)> = OnceCell::new();

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

        // Initialize lighting shader program
        let _ = LIGHTING_SHADER_PROGRAM.get_or_init(|| {
            // build and compile lighting shader program
            let vertex_shader = crate::shader::build(
                LIGHTING_VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            let fragment_shader = crate::shader::build(
                LIGHTING_FRAGMENT_SHADER_SOURCE,
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );

            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);

                let uniforms = LightingUniforms {
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(shader.gl_id, c_str!("projection").as_ptr()),
                    light_pos_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightPos").as_ptr()),
                    light_color_intensity_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightColorIntensity").as_ptr()),
                    light_direction_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightDirection").as_ptr()),
                    light_inner_cone_angle_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightInnerConeAngle").as_ptr()),
                    light_outer_cone_angle_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightOuterConeAngle").as_ptr()),
                    light_range_loc: gl::GetUniformLocation(shader.gl_id, c_str!("lightRange").as_ptr()),
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

    fn draw_light_pass(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        light: &dyn Light,
        _shadow_map: Option<&()>,
    ) -> bool {
        // Only render lighting for non-transparent materials
        if self.is_transparent() {
            return false;
        }

        // Only support spotlight for now
        if light.light_type() != LightType::Spotlight {
            return false;
        }

        let (shader_program, uniforms) = LIGHTING_SHADER_PROGRAM.get().expect("lighting shader not compiled");
        self.diffuse_texture.bind0(&render_context);

        unsafe {
            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;

            // Set basic matrices
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());

            // Set light parameters
            let light_pos = light.position();
            let light_color_intensity = light.color_intensity();
            gl::Uniform3f(uniforms.light_pos_loc, light_pos.x, light_pos.y, light_pos.z);
            gl::Uniform4f(
                uniforms.light_color_intensity_loc,
                light_color_intensity.x,
                light_color_intensity.y,
                light_color_intensity.z,
                light_color_intensity.w,
            );

            // Set spotlight-specific parameters
            if let Some(spotlight_params) = light.spotlight_params() {
                gl::Uniform3f(
                    uniforms.light_direction_loc,
                    spotlight_params.direction.x,
                    spotlight_params.direction.y,
                    spotlight_params.direction.z,
                );
                gl::Uniform1f(uniforms.light_inner_cone_angle_loc, spotlight_params.inner_cone_angle);
                gl::Uniform1f(uniforms.light_outer_cone_angle_loc, spotlight_params.outer_cone_angle);
                gl::Uniform1f(uniforms.light_range_loc, spotlight_params.range);
            }
        }

        true
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
