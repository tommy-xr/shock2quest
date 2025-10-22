extern crate gl;
use std::ops::Deref;

use crate::engine::EngineRenderContext;
use crate::scene::light_system::{LightingBatch, MAX_SPOT_LIGHTS};
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
        layout (location = 2) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec2 texCoord;
        out vec3 worldPos;
        out vec3 worldNormal;

        void main() {
            texCoord = inTex;
            vec4 worldPosition = world * vec4(inPos, 1.0);
            worldPos = worldPosition.xyz;
            worldNormal = normalize(mat3(world) * inNormal);
            gl_Position = projection * view * worldPosition;
        }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
        const int MAX_SPOT_LIGHTS = 2;

        out vec4 fragColor;

        in vec2 texCoord;
        in vec3 worldPos;
        in vec3 worldNormal;

        uniform sampler2D texture1;
        uniform float emissivity;
        uniform float transparency;

        uniform int spotLightCount;
        uniform vec3 spotLightPositions[MAX_SPOT_LIGHTS];
        uniform float spotLightRanges[MAX_SPOT_LIGHTS];
        uniform vec4 spotLightColorIntensity[MAX_SPOT_LIGHTS];
        uniform vec3 spotLightDirections[MAX_SPOT_LIGHTS];
        uniform vec2 spotLightCosAngles[MAX_SPOT_LIGHTS];

        float computeConeAttenuation(float spotFactor, float cosInner, float cosOuter) {
            if (spotFactor < cosOuter) {
                return 0.0;
            }

            if (spotFactor >= cosInner) {
                return 1.0;
            }

            float coneRange = max(cosInner - cosOuter, 0.0001);
            return (spotFactor - cosOuter) / coneRange;
        }

        void main() {
            vec4 texColor = texture(texture1, texCoord);
            if (texColor.a < 0.1) discard;

            vec3 normal = normalize(worldNormal);

            vec3 dynamicLighting = vec3(0.0);
            for (int i = 0; i < MAX_SPOT_LIGHTS; ++i) {
                if (i >= spotLightCount) {
                    break;
                }

                vec3 lightVec = spotLightPositions[i] - worldPos;
                float distance = length(lightVec);
                if (distance > spotLightRanges[i]) {
                    continue;
                }

                vec3 lightDir = normalize(lightVec);
                float spotFactor = dot(-lightDir, normalize(spotLightDirections[i]));
                float cosInner = spotLightCosAngles[i].x;
                float cosOuter = spotLightCosAngles[i].y;

                float coneAttenuation = computeConeAttenuation(spotFactor, cosInner, cosOuter);
                if (coneAttenuation <= 0.0) {
                    continue;
                }

                float distanceAttenuation = 1.0 / (1.0 + 0.1 * distance + 0.01 * distance * distance);
                float lambertian = max(dot(normal, lightDir), 0.0);

                vec3 lightContribution = texColor.rgb * spotLightColorIntensity[i].rgb * spotLightColorIntensity[i].a
                                       * lambertian * coneAttenuation * distanceAttenuation;
                dynamicLighting += lightContribution;
            }

            vec3 baseColor = texColor.rgb * (0.5 + emissivity);
            vec3 finalColor = baseColor + dynamicLighting;
            float finalAlpha = texColor.a * (1.0 - transparency);

            fragColor = vec4(finalColor, finalAlpha);
        }
"#;

struct Uniforms {
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    emissivity_loc: i32,
    transparency_loc: i32,
    spot_count_loc: i32,
    spot_positions_loc: i32,
    spot_ranges_loc: i32,
    spot_color_intensity_loc: i32,
    spot_directions_loc: i32,
    spot_cos_angles_loc: i32,
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
        lighting: &LightingBatch,
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

            gl::Uniform1i(uniforms.spot_count_loc, lighting.spot_count as i32);

            let mut spot_positions = [[0.0f32; 3]; MAX_SPOT_LIGHTS];
            let mut spot_ranges = [0.0f32; MAX_SPOT_LIGHTS];
            let mut spot_colors = [[0.0f32; 4]; MAX_SPOT_LIGHTS];
            let mut spot_directions = [[0.0f32; 3]; MAX_SPOT_LIGHTS];
            let mut spot_cos_angles = [[0.0f32; 2]; MAX_SPOT_LIGHTS];

            for i in 0..MAX_SPOT_LIGHTS {
                if i < lighting.spot_count {
                    let spot = &lighting.spots[i];
                    spot_positions[i] = [spot.position.x, spot.position.y, spot.position.z];
                    spot_ranges[i] = spot.range;
                    spot_colors[i] = [
                        spot.color_intensity.x,
                        spot.color_intensity.y,
                        spot.color_intensity.z,
                        spot.color_intensity.w,
                    ];
                    spot_directions[i] = [spot.direction.x, spot.direction.y, spot.direction.z];
                    spot_cos_angles[i] = [spot.cos_inner, spot.cos_outer];
                } else {
                    spot_ranges[i] = 0.0;
                    spot_positions[i] = [0.0; 3];
                    spot_colors[i] = [0.0; 4];
                    spot_directions[i] = [0.0; 3];
                    spot_cos_angles[i] = [0.0; 2];
                }
            }

            gl::Uniform3fv(
                uniforms.spot_positions_loc,
                MAX_SPOT_LIGHTS as i32,
                spot_positions.as_ptr() as *const f32,
            );
            gl::Uniform1fv(
                uniforms.spot_ranges_loc,
                MAX_SPOT_LIGHTS as i32,
                spot_ranges.as_ptr(),
            );
            gl::Uniform4fv(
                uniforms.spot_color_intensity_loc,
                MAX_SPOT_LIGHTS as i32,
                spot_colors.as_ptr() as *const f32,
            );
            gl::Uniform3fv(
                uniforms.spot_directions_loc,
                MAX_SPOT_LIGHTS as i32,
                spot_directions.as_ptr() as *const f32,
            );
            gl::Uniform2fv(
                uniforms.spot_cos_angles_loc,
                MAX_SPOT_LIGHTS as i32,
                spot_cos_angles.as_ptr() as *const f32,
            );
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
                    spot_count_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightCount").as_ptr(),
                    ),
                    spot_positions_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightPositions").as_ptr(),
                    ),
                    spot_ranges_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightRanges").as_ptr(),
                    ),
                    spot_color_intensity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightColorIntensity").as_ptr(),
                    ),
                    spot_directions_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightDirections").as_ptr(),
                    ),
                    spot_cos_angles_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("spotLightCosAngles").as_ptr(),
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
        lighting: &LightingBatch,
    ) -> bool {
        if !self.is_transparent() {
            self.draw_common(render_context, view_matrix, world_matrix, lighting);
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
            let empty = LightingBatch::default();
            self.draw_common(render_context, view_matrix, world_matrix, &empty);
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
