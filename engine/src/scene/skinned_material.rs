extern crate gl;
use std::rc::Rc;

use super::incidence::{IncidencePass, IncidenceUniforms};
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use crate::texture::TextureTrait;
use c_string::*;
use cgmath::Matrix4;
use cgmath::Vector3;
use cgmath::prelude::*;

use once_cell::sync::OnceCell;
use std::any::Any;

// Unified shader for single-pass lighting with up to 6 spotlights (skinned version)
const UNIFIED_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;
        layout (location = 2) in ivec4 bone_ids;
        layout (location = 3) in vec4 bone_weights;
        layout (location = 4) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform mat3 normalMatrix;
        // Affine bones packed as 3 vec4 rows each (the mat3x4's columns hold
        // the matrix rows), so the 80-slot palette costs 240 uniform vectors
        // and fits the GLES 3.0 guaranteed vertex budget (256); a mat4 array
        // would need 320. Apply with `vec4(v) * bone_matrices[i]`.
        uniform mat3x4 bone_matrices[80];

        out vec2 texCoord;
        out vec3 worldPos;
        out vec3 worldNormal;

        void main() {
            texCoord = inTex;

            // Apply weighted bone transformations to position and normal
            vec3 skinnedPos = vec3(0.0);
            vec3 skinnedNormal = vec3(0.0);
            float totalWeight = 0.0;

            // Blend up to 4 bones based on weights
            if (bone_weights.x > 0.0) {
                skinnedPos += bone_weights.x * (vec4(inPos, 1.0) * bone_matrices[bone_ids.x]);
                skinnedNormal += bone_weights.x * (vec4(inNormal, 0.0) * bone_matrices[bone_ids.x]);
                totalWeight += bone_weights.x;
            }
            if (bone_weights.y > 0.0) {
                skinnedPos += bone_weights.y * (vec4(inPos, 1.0) * bone_matrices[bone_ids.y]);
                skinnedNormal += bone_weights.y * (vec4(inNormal, 0.0) * bone_matrices[bone_ids.y]);
                totalWeight += bone_weights.y;
            }
            if (bone_weights.z > 0.0) {
                skinnedPos += bone_weights.z * (vec4(inPos, 1.0) * bone_matrices[bone_ids.z]);
                skinnedNormal += bone_weights.z * (vec4(inNormal, 0.0) * bone_matrices[bone_ids.z]);
                totalWeight += bone_weights.z;
            }
            if (bone_weights.w > 0.0) {
                skinnedPos += bone_weights.w * (vec4(inPos, 1.0) * bone_matrices[bone_ids.w]);
                skinnedNormal += bone_weights.w * (vec4(inNormal, 0.0) * bone_matrices[bone_ids.w]);
                totalWeight += bone_weights.w;
            }

            // Fallback to original position if no valid bones
            vec4 mod_position = (totalWeight > 0.0) ? vec4(skinnedPos, 1.0) : vec4(inPos, 1.0);
            vec3 mod_normal = (length(skinnedNormal) > 0.0) ? normalize(skinnedNormal) : inNormal;

            // Transform to world space
            vec4 worldPosition = world * mod_position;
            worldPos = worldPosition.xyz;
            worldNormal = normalize(normalMatrix * mod_normal);

            gl_Position = projection * view * worldPosition;
        }
"#;

pub(crate) const UNIFIED_FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in vec3 worldPos;
        in vec3 worldNormal;

        // Material properties
        uniform sampler2D texture1;
        uniform float emissivity;
        uniform float ambientIntensity;
        uniform float transparency;
        uniform vec3 silhouetteColor;
        uniform vec3 eyePosition;

        // A light painted onto part of the mesh: the mask's red channel says
        // where, `emissiveTint` says what colour and how bright. A zero tint
        // (the default) leaves the mesh exactly as it was, so meshes without a
        // light bind their own diffuse map here and pay only the sample.
        uniform sampler2D emissiveMask;
        uniform vec3 emissiveTint;

        // Spotlight array uniforms (up to 6 spotlights)
        uniform vec3 spotlightPos[6];
        uniform vec4 spotlightColorIntensity[6];  // RGB + intensity
        uniform vec3 spotlightDirection[6];
        uniform float spotlightInnerAngle[6];
        uniform float spotlightOuterAngle[6];
        uniform float spotlightRange[6];

        // Light every surface gets before any lamp reaches it.
        uniform vec3 ambientLight;
        // 0 = the renderer's smooth curve, 1 = inverse distance (how the
        // original lit objects - it falls off far more slowly).
        uniform int lightFalloffMode[6];
        // 0 = plain lambert, 1 = half-lambert (light wraps past the terminator).
        uniform float lambertWrap;

        // Lights are sources of this radius, not points: without a floor the
        // inverse-distance falloff runs away inside the model a lamp belongs
        // to. Must equal engine::scene::light::LIGHT_SOURCE_RADIUS.
        const float SOURCE_RADIUS = 0.8;

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

            // Cone attenuation. A negative inner angle marks a point light -
            // it has no cone, so it lights every direction equally.
            float coneAttenuation = 1.0;
            if (spotlightInnerAngle[i] >= 0.0) {
                float cosOuterCone = cos(spotlightOuterAngle[i]);
                float cosInnerCone = cos(spotlightInnerAngle[i]);
                float spotFactor = dot(-lightDir, normalize(spotlightDirection[i]));

                if (spotFactor < cosOuterCone) {
                    return vec3(0.0);
                }

                if (spotFactor < cosInnerCone) {
                    coneAttenuation = (spotFactor - cosOuterCone) / (cosInnerCone - cosOuterCone);
                }
            }

            // Distance attenuation
            float distanceAttenuation;
            if (lightFalloffMode[i] == 1) {
                distanceAttenuation = 1.0 / max(distance, SOURCE_RADIUS);
            } else {
                distanceAttenuation = 1.0 / (1.0 + 0.1 * distance + 0.01 * distance * distance);
            }

            // Diffuse lighting, optionally wrapped past the terminator so a
            // surface facing away is lifted rather than black.
            float ndl = dot(normal, lightDir);
            float wrap = lightFalloffMode[i] == 1 ? lambertWrap : 0.0;
            float lambertian = max((ndl + wrap) / (1.0 + wrap), 0.0);

            // Combine all factors
            return texColor * spotlightColorIntensity[i].rgb * spotlightColorIntensity[i].w
                   * lambertian * coneAttenuation * distanceAttenuation;
        }

        void main() {
            vec4 texColor = texture(texture1, texCoord);
            if (texColor.a < 0.1) discard;

            if (silhouetteColor.x >= 0.0) {
                float rim = pow(1.0 - abs(dot(normalize(worldNormal), normalize(eyePosition - worldPos))), 2.0);
                fragColor = vec4(silhouetteColor, texColor.a * (1.0 - transparency) * (0.18 + 0.82 * rim));
                return;
            }

            // Base material color (ambient)
            vec3 finalColor = texColor.rgb * ambientLight * ambientIntensity;

            // Add emissive contribution
            finalColor += texColor.rgb * emissivity;

            // Add the painted-on light. The branch is uniform across the draw,
            // so a mesh without a light never pays the second fetch.
            if (any(greaterThan(emissiveTint, vec3(0.0)))) {
                finalColor += texture(emissiveMask, texCoord).r * emissiveTint;
            }

            // Calculate contribution from all 6 spotlights
            vec3 normal = normalize(worldNormal);
            for (int i = 0; i < 6; i++) {
                finalColor += calculateSpotlight(i, worldPos, normal, texColor.rgb);
            }

            fragColor = applyIncidence(vec4(finalColor, texColor.a * (1.0 - transparency)), texColor, worldPos, worldNormal);
        }
"#;

struct UnifiedUniforms {
    incidence: IncidenceUniforms,
    // Basic transformation matrices
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    normal_matrix_loc: i32,

    // Material properties
    emissivity_loc: i32,
    ambient_intensity_loc: i32,
    transparency_loc: i32,
    emissive_tint_loc: i32,
    silhouette_color_loc: i32,
    eye_position_loc: i32,

    // Bone matrices for skeletal animation
    bone_matrices_loc: i32,

    // Spotlight array uniforms (6 spotlights)
    spotlight_pos_loc: [i32; 6],
    spotlight_color_intensity_loc: [i32; 6],
    spotlight_direction_loc: [i32; 6],
    spotlight_inner_angle_loc: [i32; 6],
    spotlight_outer_angle_loc: [i32; 6],
    spotlight_range_loc: [i32; 6],
    ambient_light_loc: i32,
    light_falloff_mode_loc: i32,
    lambert_wrap_loc: i32,
}

static UNIFIED_SHADER_PROGRAM: OnceCell<(ShaderProgram, UnifiedUniforms)> = OnceCell::new();

#[derive(Clone)]
pub struct SkinnedMaterial {
    incidence: Option<IncidencePass>,
    silhouette_color: Option<Vector3<f32>>,
    has_initialized: bool,
    diffuse_texture: Rc<dyn TextureTrait>,
    emissivity: f32,
    transparency: f32,
    base_transparency: f32,
    /// Where the painted-on light sits, in the diffuse map's own UVs (red
    /// channel). `None` binds the diffuse map in its place, so the sampler
    /// always has a real texture; `emissive_tint` is then zero and the sample
    /// contributes nothing.
    emissive_mask: Option<Rc<dyn TextureTrait>>,
    emissive_tint: Vector3<f32>,
}

impl SkinnedMaterial {
    /// Reuse the animated mesh and skinning shader for an unlit rim silhouette.
    /// The source material and other instances of the model stay unchanged.
    pub fn silhouette(&self, color: Vector3<f32>) -> Box<dyn Material> {
        let mut material = self.clone();
        material.silhouette_color = Some(color);
        material.incidence = None;
        Box::new(material)
    }

    pub fn is_transparent(&self) -> bool {
        self.incidence.is_some() || self.transparency > 0.01
    }

    pub fn set_transparency_override(&mut self, transparency: f32) {
        self.transparency = transparency.clamp(0.0, 1.0);
    }

    pub fn reset_transparency(&mut self) {
        self.transparency = self.base_transparency;
    }

    pub fn draw_unified(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        skinning_data: &[Matrix4<f32>],
        lights: &crate::scene::light::LightArray,
    ) {
        let (shader_program, uniforms) = UNIFIED_SHADER_PROGRAM
            .get()
            .expect("unified shader not compiled");
        self.diffuse_texture.bind0(render_context);
        self.emissive_mask
            .as_ref()
            .unwrap_or(&self.diffuse_texture)
            .bind1(render_context);
        unsafe {
            gl::UseProgram(shader_program.gl_id);
            uniforms
                .incidence
                .bind(self.incidence.as_ref(), render_context, view_matrix);

            let projection = render_context.projection_matrix;

            // Set basic transformation matrices
            gl::UniformMatrix4fv(uniforms.world_loc, 1, gl::FALSE, world_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.view_loc, 1, gl::FALSE, view_matrix.as_ptr());
            gl::UniformMatrix4fv(uniforms.projection_loc, 1, gl::FALSE, projection.as_ptr());
            let normal_matrix = crate::scene::material::normal_matrix(world_matrix);
            gl::UniformMatrix3fv(
                uniforms.normal_matrix_loc,
                1,
                gl::FALSE,
                normal_matrix.as_ptr(),
            );

            let color = self
                .silhouette_color
                .unwrap_or(Vector3::new(-1.0, -1.0, -1.0));
            gl::Uniform3fv(uniforms.silhouette_color_loc, 1, color.as_ptr());
            if self.silhouette_color.is_some() {
                let eye = view_matrix
                    .invert()
                    .unwrap_or_else(Matrix4::identity)
                    .w
                    .truncate();
                gl::Uniform3fv(uniforms.eye_position_loc, 1, eye.as_ptr());
            }

            // Set material properties
            gl::Uniform1f(uniforms.transparency_loc, self.transparency);
            gl::Uniform1f(uniforms.emissivity_loc, self.emissivity);
            gl::Uniform1f(
                uniforms.ambient_intensity_loc,
                render_context.ambient_light_intensity,
            );
            gl::Uniform3f(
                uniforms.emissive_tint_loc,
                self.emissive_tint.x,
                self.emissive_tint.y,
                self.emissive_tint.z,
            );

            // Pack each affine bone as its 3 matrix rows (the shader-side
            // mat3x4's columns) and upload the whole palette in one call.
            let mut packed = [0f32; crate::scene::SKINNING_PALETTE_SIZE * 12];
            for (bone, mat) in skinning_data
                .iter()
                .enumerate()
                .take(crate::scene::SKINNING_PALETTE_SIZE)
            {
                let base = bone * 12;
                for row in 0..3 {
                    packed[base + row * 4] = mat.x[row];
                    packed[base + row * 4 + 1] = mat.y[row];
                    packed[base + row * 4 + 2] = mat.z[row];
                    packed[base + row * 4 + 3] = mat.w[row];
                }
            }
            gl::UniformMatrix3x4fv(
                uniforms.bone_matrices_loc,
                crate::scene::SKINNING_PALETTE_SIZE as i32,
                gl::FALSE,
                packed.as_ptr(),
            );

            // How this array's lights behave: what an unlit surface shows, and
            // how the lights fall off.
            gl::Uniform3f(
                uniforms.ambient_light_loc,
                lights.ambient.x,
                lights.ambient.y,
                lights.ambient.z,
            );
            gl::Uniform1f(uniforms.lambert_wrap_loc, lights.lambert_wrap);
            let falloff_modes = lights.falloff.map(|mode| match mode {
                crate::scene::light::LightFalloff::Smooth => 0,
                crate::scene::light::LightFalloff::InverseDistance => 1,
            });
            gl::Uniform1iv(uniforms.light_falloff_mode_loc, 6, falloff_modes.as_ptr());

            // Set spotlight array uniforms
            for i in 0..6 {
                if let Some(light) = lights.get_light(i) {
                    let pos = light.position();
                    let color_intensity = light.color_intensity();
                    let direction = light.direction();

                    gl::Uniform3f(uniforms.spotlight_pos_loc[i], pos.x, pos.y, pos.z);
                    gl::Uniform4f(
                        uniforms.spotlight_color_intensity_loc[i],
                        color_intensity.x,
                        color_intensity.y,
                        color_intensity.z,
                        color_intensity.w,
                    );
                    gl::Uniform3f(
                        uniforms.spotlight_direction_loc[i],
                        direction.x,
                        direction.y,
                        direction.z,
                    );
                    gl::Uniform1f(
                        uniforms.spotlight_inner_angle_loc[i],
                        light.inner_cone_angle(),
                    );
                    gl::Uniform1f(
                        uniforms.spotlight_outer_angle_loc[i],
                        light.outer_cone_angle(),
                    );
                    gl::Uniform1f(uniforms.spotlight_range_loc[i], light.range());
                } else {
                    // Disable this light slot by setting intensity to 0
                    gl::Uniform4f(
                        uniforms.spotlight_color_intensity_loc[i],
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    );
                }
            }
        }
    }
}
impl Material for SkinnedMaterial {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_transparency_override(&mut self, transparency: Option<f32>) {
        self.transparency = match transparency {
            Some(value) => value.clamp(0.0, 1.0),
            None => self.base_transparency,
        };
    }

    fn transparency(&self) -> Option<f32> {
        Some(self.transparency)
    }

    fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    fn initialize(&mut self, is_opengl_es: bool) {
        let _ = UNIFIED_SHADER_PROGRAM.get_or_init(|| {
            // Build and compile unified shader program with 6-spotlight support for skinned meshes
            let vertex_shader = crate::shader::build(
                UNIFIED_VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            let fragment_shader = crate::shader::build(
                &format!(
                    "{}\n{}",
                    super::incidence::GLSL,
                    UNIFIED_FRAGMENT_SHADER_SOURCE
                ),
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );

            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);

                // Which texture unit each sampler reads is program state, not
                // per-material state, so it is set once here rather than on
                // every draw. (`texture1` would default to unit 0 anyway; the
                // mask has to be told about unit 1.)
                gl::UseProgram(shader.gl_id);
                gl::Uniform1i(
                    gl::GetUniformLocation(shader.gl_id, c_str!("texture1").as_ptr()),
                    0,
                );
                gl::Uniform1i(
                    gl::GetUniformLocation(shader.gl_id, c_str!("emissiveMask").as_ptr()),
                    1,
                );

                // Get uniform locations for all shader variables
                let bone_matrices_loc =
                    gl::GetUniformLocation(shader.gl_id, c_str!("bone_matrices[0]").as_ptr());

                let uniforms = UnifiedUniforms {
                    incidence: IncidenceUniforms::new(shader.gl_id),
                    // Basic transformation matrices
                    world_loc: gl::GetUniformLocation(shader.gl_id, c_str!("world").as_ptr()),
                    view_loc: gl::GetUniformLocation(shader.gl_id, c_str!("view").as_ptr()),
                    projection_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("projection").as_ptr(),
                    ),
                    normal_matrix_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("normalMatrix").as_ptr(),
                    ),

                    // Material properties
                    ambient_intensity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("ambientIntensity").as_ptr(),
                    ),
                    emissivity_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("emissivity").as_ptr(),
                    ),
                    transparency_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("transparency").as_ptr(),
                    ),
                    emissive_tint_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("emissiveTint").as_ptr(),
                    ),

                    silhouette_color_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("silhouetteColor").as_ptr(),
                    ),
                    eye_position_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("eyePosition").as_ptr(),
                    ),

                    // Bone matrices
                    bone_matrices_loc,

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
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[0]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[1]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[2]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[3]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[4]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightColorIntensity[5]").as_ptr(),
                        ),
                    ],
                    spotlight_direction_loc: [
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[0]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[1]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[2]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[3]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[4]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightDirection[5]").as_ptr(),
                        ),
                    ],
                    spotlight_inner_angle_loc: [
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[0]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[1]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[2]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[3]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[4]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightInnerAngle[5]").as_ptr(),
                        ),
                    ],
                    spotlight_outer_angle_loc: [
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[0]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[1]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[2]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[3]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[4]").as_ptr(),
                        ),
                        gl::GetUniformLocation(
                            shader.gl_id,
                            c_str!("spotlightOuterAngle[5]").as_ptr(),
                        ),
                    ],
                    spotlight_range_loc: [
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[0]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[1]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[2]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[3]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[4]").as_ptr()),
                        gl::GetUniformLocation(shader.gl_id, c_str!("spotlightRange[5]").as_ptr()),
                    ],
                    ambient_light_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("ambientLight").as_ptr(),
                    ),
                    light_falloff_mode_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("lightFalloffMode[0]").as_ptr(),
                    ),
                    lambert_wrap_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("lambertWrap").as_ptr(),
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
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        if !self.is_transparent() {
            self.draw_unified(
                render_context,
                view_matrix,
                world_matrix,
                _skinning_data,
                _lights,
            );
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
        _lights: &crate::scene::light::LightArray,
    ) -> bool {
        if self.is_transparent() {
            self.draw_unified(
                render_context,
                view_matrix,
                world_matrix,
                _skinning_data,
                _lights,
            );
            true
        } else {
            false
        }
    }
}

impl SkinnedMaterial {
    pub fn create(
        diffuse_texture: Rc<dyn TextureTrait>,
        emissivity: f32,
        transparency: f32,
    ) -> Box<dyn Material> {
        Self::build(
            diffuse_texture,
            emissivity,
            transparency,
            None,
            Vector3::new(0.0, 0.0, 0.0),
        )
    }

    /// As [`SkinnedMaterial::create`], plus a light painted onto the parts of
    /// the mesh `emissive_mask` marks (its red channel), in `emissive_tint`'s
    /// colour. A zero tint renders identically to `create`.
    pub fn create_with_light(
        diffuse_texture: Rc<dyn TextureTrait>,
        emissivity: f32,
        transparency: f32,
        emissive_mask: Rc<dyn TextureTrait>,
        emissive_tint: Vector3<f32>,
    ) -> Box<dyn Material> {
        Self::build(
            diffuse_texture,
            emissivity,
            transparency,
            Some(emissive_mask),
            emissive_tint,
        )
    }

    /// Use the normal skinning palette and lighting for a specular overlay.
    pub fn create_incidence(
        texture: Rc<dyn TextureTrait>,
        pass: IncidencePass,
    ) -> Box<dyn Material> {
        Box::new(Self {
            incidence: Some(pass),
            silhouette_color: None,
            diffuse_texture: texture,
            has_initialized: false,
            emissivity: 0.0,
            transparency: 0.0,
            base_transparency: 0.0,
            emissive_mask: None,
            emissive_tint: Vector3::new(0.0, 0.0, 0.0),
        })
    }

    fn build(
        diffuse_texture: Rc<dyn TextureTrait>,
        emissivity: f32,
        transparency: f32,
        emissive_mask: Option<Rc<dyn TextureTrait>>,
        emissive_tint: Vector3<f32>,
    ) -> Box<dyn Material> {
        Box::new(SkinnedMaterial {
            incidence: None,
            silhouette_color: None,
            diffuse_texture,
            has_initialized: false,
            emissivity,
            transparency,
            base_transparency: transparency,
            emissive_mask,
            emissive_tint,
        })
    }
}

/// The fragment shader source, for the test that pins its SOURCE_RADIUS to the
/// engine constant. Both lit materials declare it; both must agree.
#[cfg(test)]
pub(crate) const FRAGMENT_SHADER_SOURCE_FOR_TEST: &str = UNIFIED_FRAGMENT_SHADER_SOURCE;
