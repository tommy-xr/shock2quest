extern crate gl;
use crate::engine::EngineRenderContext;
use crate::scene::light::{Light, LightType};
use crate::scene::Material;
use crate::shader_program::ShaderProgram;
use crate::texture::Texture;
use crate::texture::TextureTrait;
use c_string::*;
use cgmath::prelude::*;

use cgmath::Matrix4;
use once_cell::sync::OnceCell;
use std::rc::Rc;

// Unified shader for single-pass lighting with lightmaps + 6 dynamic spotlights
const UNIFIED_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;
        layout (location = 2) in vec2 inLightMapTex;
        layout (location = 3) in vec4 inAtlas;
        layout (location = 4) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;

        out vec2 texCoord;
        out highp vec2 lightMapTexCoord;
        out highp vec4 atlasCoord;
        out vec3 worldPos;
        out vec3 worldNormal;

        void main() {
            texCoord = inTex;
            lightMapTexCoord = inLightMapTex;
            atlasCoord = inAtlas;

            vec4 worldPosition = world * vec4(inPos, 1.0);
            worldPos = worldPosition.xyz;
            worldNormal = normalize(mat3(world) * inNormal);

            gl_Position = projection * view * worldPosition;
        }
"#;

const UNIFIED_FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in highp vec2 lightMapTexCoord;
        in highp vec4 atlasCoord;
        in vec3 worldPos;
        in vec3 worldNormal;

        // Material properties
        uniform sampler2D texture1; // lightmap
        uniform sampler2D texture2; // diffuse texture

        // Spotlight array uniforms (up to 6 spotlights)
        uniform vec3 spotlightPos[6];
        uniform vec4 spotlightColorIntensity[6];  // RGB + intensity
        uniform vec3 spotlightDirection[6];
        uniform float spotlightInnerAngle[6];
        uniform float spotlightOuterAngle[6];
        uniform float spotlightRange[6];

        // Calculate spotlight contribution
        vec3 calculateSpotlight(int i, vec3 worldPos, vec3 normal, vec3 texColor) {
            // Skip if light has zero intensity
            if (spotlightColorIntensity[i].w <= 0.0) {
                return vec3(0.0);
            }

            vec3 lightVec = spotlightPos[i] - worldPos;
            float distance = length(lightVec);

            // Range check
            if (distance > spotlightRange[i]) {
                return vec3(0.0);
            }

            vec3 lightDir = normalize(lightVec);

            // Cone attenuation for spotlight
            float cosOuterCone = cos(spotlightOuterAngle[i]);
            float cosInnerCone = cos(spotlightInnerAngle[i]);
            float spotFactor = dot(-lightDir, normalize(spotlightDirection[i]));

            if (spotFactor < cosOuterCone) {
                return vec3(0.0);
            }

            float coneAttenuation = 1.0;
            if (spotFactor < cosInnerCone) {
                coneAttenuation = (spotFactor - cosOuterCone) / (cosInnerCone - cosOuterCone);
            }

            // Distance attenuation
            float distanceAttenuation = 1.0 / (1.0 + 0.1 * distance + 0.01 * distance * distance);

            // Diffuse lighting
            float lambertian = max(dot(normal, lightDir), 0.0);

            // Combine all factors
            return texColor * spotlightColorIntensity[i].rgb * spotlightColorIntensity[i].w
                   * lambertian * coneAttenuation * distanceAttenuation;
        }

        void main() {
            // Sample lightmap and diffuse texture with proper UV wrapping
            float half_pixel = 0.5 / 4096.0;
            float full_pixel = half_pixel * 2.0;
            vec2 wrappedTexCoord = vec2(0.0, 0.0);
            float width = atlasCoord.z - full_pixel;
            float height = atlasCoord.w - full_pixel;

            wrappedTexCoord.x = mod(lightMapTexCoord.x * width, width) + atlasCoord.x + half_pixel;
            wrappedTexCoord.y = mod(lightMapTexCoord.y * height, height) + atlasCoord.y + half_pixel;

            vec4 lightmapColor = texture(texture1, wrappedTexCoord);
            vec4 diffuseColor = texture(texture2, texCoord);

            // Base lighting from lightmap (baked static lighting)
            vec3 finalColor = diffuseColor.rgb * lightmapColor.rgb;

            // Add dynamic spotlight contributions on top of baked lighting
            vec3 normal = normalize(worldNormal);
            for (int i = 0; i < 6; i++) {
                finalColor += calculateSpotlight(i, worldPos, normal, diffuseColor.rgb);
            }

            fragColor = vec4(finalColor, 1.0);
        }
"#;


struct UnifiedUniforms {
    // Basic transformation matrices
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,

    // Texture samplers
    texture1_loc: i32, // lightmap
    texture2_loc: i32, // diffuse

    // Spotlight array uniforms (6 spotlights)
    spotlight_pos_loc: [i32; 6],
    spotlight_color_intensity_loc: [i32; 6],
    spotlight_direction_loc: [i32; 6],
    spotlight_inner_angle_loc: [i32; 6],
    spotlight_outer_angle_loc: [i32; 6],
    spotlight_range_loc: [i32; 6],
}

static UNIFIED_SHADER_PROGRAM: OnceCell<(ShaderProgram, UnifiedUniforms)> = OnceCell::new();

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

    pub fn draw_unified(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        let (shader_program, uniforms) = UNIFIED_SHADER_PROGRAM.get().expect("unified shader not compiled");

        unsafe {
            // Bind textures
            crate::texture::bind0(&self.lightmap_texture);
            self.diffuse_texture.bind1(render_context);

            gl::UseProgram(shader_program.gl_id);

            let projection = render_context.projection_matrix;

            // Set basic transformation matrices
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());

            // Set texture samplers
            gl::Uniform1i(uniforms.texture1_loc, 0); // lightmap
            gl::Uniform1i(uniforms.texture2_loc, 1); // diffuse

            // Set spotlight array uniforms
            for i in 0..6 {
                if let Some(spotlight) = lights.get_spotlight(i) {
                    let pos = spotlight.position();
                    let color_intensity = spotlight.color_intensity();
                    let direction = spotlight.direction;

                    gl::Uniform3f(uniforms.spotlight_pos_loc[i], pos.x, pos.y, pos.z);
                    gl::Uniform4f(
                        uniforms.spotlight_color_intensity_loc[i],
                        color_intensity.x,
                        color_intensity.y,
                        color_intensity.z,
                        color_intensity.w,
                    );
                    gl::Uniform3f(uniforms.spotlight_direction_loc[i], direction.x, direction.y, direction.z);
                    gl::Uniform1f(uniforms.spotlight_inner_angle_loc[i], spotlight.inner_cone_angle);
                    gl::Uniform1f(uniforms.spotlight_outer_angle_loc[i], spotlight.outer_cone_angle);
                    gl::Uniform1f(uniforms.spotlight_range_loc[i], spotlight.range);
                } else {
                    // Disable this light slot by setting intensity to 0
                    gl::Uniform4f(uniforms.spotlight_color_intensity_loc[i], 0.0, 0.0, 0.0, 0.0);
                }
            }
        }
    }
}

impl Material for LightmapMaterial {
    fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    fn initialize(&mut self, is_opengl_es: bool, _storage: &dyn crate::file_system::Storage) {
        let _ = UNIFIED_SHADER_PROGRAM.get_or_init(|| {
            // Build and compile unified shader program with lightmaps + 6-spotlight support
            let vertex_shader = crate::shader::build(
                UNIFIED_VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            let fragment_shader = crate::shader::build(
                UNIFIED_FRAGMENT_SHADER_SOURCE,
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );

            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);

                // Get uniform locations for all shader variables
                let uniforms = UnifiedUniforms {
                    // Basic transformation matrices
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(shader.gl_id, c_str!("projection").as_ptr()),

                    // Texture samplers
                    texture1_loc: gl::GetUniformLocation(shader.gl_id, c_str!("texture1").as_ptr()),
                    texture2_loc: gl::GetUniformLocation(shader.gl_id, c_str!("texture2").as_ptr()),

                    // Spotlight array uniforms (6 spotlights)
                    spotlight_pos_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightPos[5]").as_ptr()),
                    ],
                    spotlight_color_intensity_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightColorIntensity[5]").as_ptr()),
                    ],
                    spotlight_direction_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightDirection[5]").as_ptr()),
                    ],
                    spotlight_inner_angle_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightInnerAngle[5]").as_ptr()),
                    ],
                    spotlight_outer_angle_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightOuterAngle[5]").as_ptr()),
                    ],
                    spotlight_range_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[5]").as_ptr()),
                    ],
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
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        self.draw_unified(render_context, view_matrix, world_matrix, lights);
        true
    }

    fn draw_transparent(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        _skinning_data: &[Matrix4<f32>],
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        // Lightmap materials are typically opaque
        false
    }
}
