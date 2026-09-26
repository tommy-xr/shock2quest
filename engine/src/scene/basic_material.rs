extern crate gl;
use std::any::Any;
use std::ops::Deref;

use super::incidence::{IncidencePass, IncidenceUniforms};
use super::shine::{Shine, ShineUniforms};
use crate::engine::EngineRenderContext;
use crate::scene::Material;
use crate::shader_program::ShaderProgram;

use crate::texture::TextureTrait;
use c_string::*;
use cgmath::Matrix4;
use cgmath::prelude::*;

use once_cell::sync::OnceCell;

// Unified shader for single-pass lighting with up to 6 spotlights
const UNIFIED_VERTEX_SHADER_SOURCE: &str = r#"
        layout (location = 0) in vec3 inPos;
        layout (location = 1) in vec2 inTex;
        layout (location = 2) in vec3 inNormal;

        uniform mat4 world;
        uniform mat4 view;
        uniform mat4 projection;
        uniform mat3 normalMatrix;

        out vec2 texCoord;
        out vec3 worldPos;
        out vec3 worldNormal;

        void main() {
            texCoord = inTex;
            vec4 worldPosition = world * vec4(inPos, 1.0);
            worldPos = worldPosition.xyz;

            worldNormal = normalize(normalMatrix * inNormal);

            gl_Position = projection * view * worldPosition;
        }
"#;

const UNIFIED_FRAGMENT_SHADER_SOURCE: &str = r#"
        out vec4 fragColor;

        in vec2 texCoord;
        in vec3 worldPos;
        in vec3 worldNormal;

        // Material properties
        uniform sampler2D texture1;
        uniform float emissivity;
        uniform float ambientIntensity;
        uniform float transparency;
        uniform bool additiveUnlit;

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
        vec3 calculateSpotlight(int i, vec3 worldPos, vec3 normal, vec3 texColor, inout vec3 specular, float power) {
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

            vec3 radiance = spotlightColorIntensity[i].rgb * spotlightColorIntensity[i].w
                            * coneAttenuation * distanceAttenuation;
            if (shineHighlights()) {
                specular += shineHighlight(radiance, lightDir, normal, worldPos, power);
            }
            return texColor * radiance * lambertian;
        }

        void main() {
            vec4 texColor = texture(texture1, texCoord);
            if (additiveUnlit) {
                // SRC_COLOR/ONE squares RGB. Scale by sqrt(opacity) so the
                // accumulated light fades linearly with authored alpha.
                fragColor = vec4(texColor.rgb * sqrt(texColor.a * (1.0 - transparency)), 1.0);
                return;
            }
            if (texColor.a < 0.1) discard;

            vec3 finalColor = texColor.rgb * emissivity;

            // Light reaching the surface before albedo, shared with the shine.
            vec3 normal = normalize(worldNormal);
            vec3 light = ambientLight * ambientIntensity;
            vec3 specular = vec3(0.0);
            float power = shineHighlights() ? shinePower(length(worldNormal)) : SHINE_POWER;
            for (int i = 0; i < 6; i++) {
                light += calculateSpotlight(i, worldPos, normal, vec3(1.0), specular, power);
            }
            finalColor += texColor.rgb * light;
            vec4 shaded = applyShine(vec4(finalColor, texColor.a * (1.0 - transparency)), light, specular, 1.0 - transparency, texCoord, worldPos, normal);

            fragColor = applyIncidence(shaded, texColor, worldPos, worldNormal);
        }
"#;

struct UnifiedUniforms {
    incidence: IncidenceUniforms,
    shine: ShineUniforms,
    // Basic transformation matrices
    world_loc: i32,
    view_loc: i32,
    projection_loc: i32,
    normal_matrix_loc: i32,

    // Material properties
    emissivity_loc: i32,
    ambient_intensity_loc: i32,
    transparency_loc: i32,
    additive_unlit_loc: i32,

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

pub struct BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait + 'static>,
{
    has_initialized: bool,
    diffuse_texture: T,
    emissivity: f32,
    transparency: f32,
    base_transparency: f32,
    additive_unlit: bool,
    fixed_ambient: bool,
    incidence: Option<IncidencePass>,
    shine: Option<Shine>,
}

impl<T> BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait>,
{
    pub fn is_transparent(&self) -> bool {
        self.incidence.is_some() || self.additive_unlit || self.transparency > 0.01
    }

    pub fn draw_unified(
        &self,
        render_context: &EngineRenderContext,
        view_matrix: &Matrix4<f32>,
        world_matrix: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        let (shader_program, uniforms) = UNIFIED_SHADER_PROGRAM
            .get()
            .expect("unified shader not compiled");
        self.diffuse_texture.bind0(render_context);
        unsafe {
            gl::UseProgram(shader_program.gl_id);
            uniforms
                .incidence
                .bind(self.incidence.as_ref(), render_context, view_matrix);
            uniforms
                .shine
                .bind(self.shine.as_ref(), lights, render_context, view_matrix);

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

            // Set material properties
            gl::Uniform1i(uniforms.additive_unlit_loc, i32::from(self.additive_unlit));
            gl::Uniform1f(uniforms.transparency_loc, self.transparency);
            gl::Uniform1f(uniforms.emissivity_loc, self.emissivity);
            gl::Uniform1f(
                uniforms.ambient_intensity_loc,
                if self.fixed_ambient {
                    1.0
                } else {
                    render_context.ambient_light_intensity
                },
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
impl<T> Material for BasicMaterial<T>
where
    T: Deref<Target = dyn TextureTrait> + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_shine(&mut self, shine: Shine) {
        self.shine = Some(shine);
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
            // Build and compile unified shader program with 6-spotlight support
            let vertex_shader = crate::shader::build(
                UNIFIED_VERTEX_SHADER_SOURCE,
                crate::shader::ShaderType::Vertex,
                is_opengl_es,
            );

            let fragment_shader = crate::shader::build(
                &format!(
                    "{}\n{}\n{}\n{}",
                    super::incidence::GLSL,
                    super::environment::GLSL,
                    super::shine::GLSL,
                    UNIFIED_FRAGMENT_SHADER_SOURCE
                ),
                crate::shader::ShaderType::Fragment,
                is_opengl_es,
            );

            unsafe {
                let shader = crate::shader_program::link(&vertex_shader, &fragment_shader);

                // Get uniform locations for all shader variables
                let uniforms = UnifiedUniforms {
                    incidence: IncidenceUniforms::new(shader.gl_id),
                    shine: ShineUniforms::new(shader.gl_id),
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
                    additive_unlit_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("additiveUnlit").as_ptr(),
                    ),
                    transparency_loc: gl::GetUniformLocation(
                        shader.gl_id,
                        c_str!("transparency").as_ptr(),
                    ),

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
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        if !self.is_transparent() {
            self.draw_unified(render_context, view_matrix, world_matrix, lights);
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
        lights: &crate::scene::light::LightArray,
    ) -> bool {
        if self.is_transparent() {
            self.draw_unified(render_context, view_matrix, world_matrix, lights);
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
    create_with_additive(diffuse_texture, emissivity, transparency, false)
}

pub fn create_with_additive<T>(
    diffuse_texture: T,
    emissivity: f32,
    transparency: f32,
    additive_unlit: bool,
) -> Box<dyn Material>
where
    T: Deref<Target = dyn TextureTrait> + 'static,
{
    Box::new(BasicMaterial {
        diffuse_texture,
        has_initialized: false,
        emissivity,
        transparency,
        base_transparency: transparency,
        additive_unlit,
        fixed_ambient: false,
        incidence: None,
        shine: None,
    })
}

/// Preserve the ambient baseline for UI, video, and explicitly fullbright surfaces.
/// These materials must stay readable independently of world lighting tuning.
pub fn create_with_fixed_ambient<T>(
    diffuse_texture: T,
    emissivity: f32,
    transparency: f32,
) -> Box<dyn Material>
where
    T: Deref<Target = dyn TextureTrait> + 'static,
{
    Box::new(BasicMaterial {
        diffuse_texture,
        has_initialized: false,
        emissivity,
        transparency,
        base_transparency: transparency,
        additive_unlit: false,
        fixed_ambient: true,
        incidence: None,
        shine: None,
    })
}

/// A depth-tested additive overlay using the same geometry and lighting as
/// the base material. SceneObject supplies SRC_ALPHA/ONE composition.
pub fn create_incidence(
    texture: std::rc::Rc<dyn TextureTrait>,
    pass: IncidencePass,
) -> Box<dyn Material> {
    Box::new(BasicMaterial {
        diffuse_texture: texture,
        has_initialized: false,
        emissivity: 0.0,
        transparency: 0.0,
        base_transparency: 0.0,
        additive_unlit: false,
        fixed_ambient: false,
        incidence: Some(pass),
        shine: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader floors its inverse-distance falloff at a source radius, and
    /// the light selection ranks with the same floor. They are declared in two
    /// languages, so pin them to each other: if they drift, the lights chosen
    /// are not the lights drawn.
    #[test]
    fn shader_source_radius_matches_the_engine_constant() {
        let declared = format!(
            "const float SOURCE_RADIUS = {};",
            crate::scene::light::LIGHT_SOURCE_RADIUS
        );
        for (name, source) in [
            ("basic", UNIFIED_FRAGMENT_SHADER_SOURCE),
            (
                "skinned",
                crate::scene::skinned_material::FRAGMENT_SHADER_SOURCE_FOR_TEST,
            ),
        ] {
            assert!(
                source.contains(&declared),
                "{name} shader must declare `{declared}`"
            );
        }
    }
}
