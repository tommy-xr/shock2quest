//! Authored view-angle shine drawn in the base pass. A 25AE material stacks
//! identical `INCIDENCE` passes over its diffuse; composing them in the same
//! fragment gives the stacked result without re-drawing (and re-skinning) the
//! mesh once per pass. Shared by static and skinned shaders.
use crate::{engine::EngineRenderContext, scene::light::LightArray, texture::TextureTrait};
use cgmath::{Matrix4, Vector3, prelude::*};
use std::rc::Rc;

/// How each authored pass composites onto the one below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShineBlend {
    /// `blend SRC_ALPHA ONE`
    Additive,
    /// `blend SRC_ALPHA INV_SRC_ALPHA`
    Alpha,
}

#[derive(Clone)]
pub struct Shine {
    /// The pass's own bitmap: colour and coverage of the shine.
    pub mask: Rc<dyn TextureTrait>,
    /// Opacity by view angle, indexed by |N·V|.
    pub ramp: Rc<dyn TextureTrait>,
    pub tint: Vector3<f32>,
    pub unlit: bool,
    pub blend: ShineBlend,
    /// How many identical passes the material stacks.
    pub passes: u32,
    /// Scales the lights' highlight strength on this surface.
    pub specular: f32,
}

/// `light` is ambient plus the lamps reaching the fragment, before albedo:
/// a lit pass shades its mask exactly as the base shades its diffuse.
/// Returns colour and coverage such that the renderer's SRC_ALPHA blend of
/// this one fragment equals the base then each pass blended in turn.
pub(crate) const GLSL: &str = r#"
uniform bool shineEnabled;
uniform sampler2D shineMask;
uniform sampler2D shineRamp;
uniform vec3 shineEye;
uniform vec3 shineTint;
uniform bool shineUnlit;
uniform bool shineAlphaBlend;
uniform float shinePasses;
uniform float shineSpecular;
uniform samplerCube shineEnvironment;
uniform float shineReflection;

// Highlight exponent. Higher reads wetter but sparkles on low-poly meshes in
// a headset.
const float SHINE_POWER = 24.0;
// Gloss away from the authored glints, relative to 1 on them.
const float SHINE_BASE_GLOSS = 0.5;
// Mip the capture is reflected at: blurry, as a curved wet surface would
// show it, and forgiving of the capture having been taken elsewhere on the deck.
const float REFLECTION_LOD = 3.0;
// Schlick reflectance head-on (above water's 0.02 so it reads at a glance);
// grazing angles approach 1.
const float REFLECTION_F0 = 0.05;

bool shineHighlights() {
    return shineEnabled && shineSpecular > 0.0;
}

// Toksvig: interpolated vertex normals shorten where the surface bends
// within a triangle, so widen the lobe there instead of letting it flicker.
float shinePower(float normalLength) {
    return SHINE_POWER * normalLength / (normalLength + SHINE_POWER * (1.0 - normalLength));
}

// Blinn-Phong lobe of one light; `radiance` is its colour after attenuation.
// The energy-conserving scale keeps a widened lobe from brightening.
vec3 shineHighlight(vec3 radiance, vec3 lightDir, vec3 normal, vec3 position, float power) {
    // The normal nudge keeps a light straight behind the surface from
    // normalizing a zero vector.
    vec3 halfway = normalize(lightDir + normalize(shineEye - position) + 1e-4 * normal);
    // Fade in past the terminator rather than cutting off at it.
    float lit = clamp(dot(normal, lightDir) * 4.0, 0.0, 1.0);
    return radiance * pow(max(dot(normal, halfway), 0.0), power) * lit
        * (1.0 + power) / (1.0 + SHINE_POWER);
}

// `specular` is the summed highlight of every light.
vec4 applyShine(vec4 base, vec3 light, vec3 specular, float opacity, vec2 uv, vec3 position, vec3 normal) {
    if (!shineEnabled) return base;
    vec4 mask = texture(shineMask, uv);
    // A separate pass would alpha-test its own bitmap.
    if (mask.a < 0.1 && !shineHighlights() && shineReflection <= 0.0) return base;
    vec3 toEye = normalize(shineEye - position);
    float facing = clamp(abs(dot(normal, toEye)), 0.0, 1.0);
    float alpha = mask.a < 0.1 ? 0.0 : mask.a * opacity * texture(shineRamp, vec2(facing, 0.5)).r;
    vec3 shine = min((shineUnlit ? mask.rgb : mask.rgb * light) * shineTint, vec3(1.0));
    // The whole surface is wet, in the mask's colour; its sparse alpha marks
    // the glossiest glints. The ramp only shapes the authored sheen.
    float gloss = mix(SHINE_BASE_GLOSS, 1.0, mask.a) * opacity;
    vec3 highlight = mask.rgb * gloss * specular * shineSpecular;
    if (shineReflection > 0.0) {
        vec3 mirrored = reflect(-toEye, normal);
        vec3 seen = textureLod(shineEnvironment, environmentDirection(mirrored), REFLECTION_LOD).rgb;
        float fresnel = mix(REFLECTION_F0, 1.0, pow(1.0 - facing, 5.0));
        // The capture is a lit photograph of the deck; the light that actually
        // reaches this surface keeps a dark room from reflecting a bright one.
        highlight += seen * mask.rgb * min(light, vec3(1.0)) * fresnel * gloss * shineReflection;
    }
    // Each stacked pass blended onto a clamped framebuffer.
    vec3 color = min(base.rgb, vec3(1.0));
    if (shineAlphaBlend) {
        float covered = 1.0 - pow(1.0 - alpha, shinePasses);
        float outAlpha = base.a * (1.0 - covered) + covered;
        vec3 blended = color * base.a * (1.0 - covered) + shine * covered + highlight;
        return vec4(blended / max(outAlpha, 1.0 / 255.0), outAlpha);
    }
    float coverage = max(base.a, 1.0 / 255.0);
    // The blend scales colour by the base's coverage; an additive pass does
    // not. Exact until a faded base's pre-divided colour saturates.
    return vec4(color + (shinePasses * shine * alpha + highlight) / coverage, base.a);
}
"#;

const MASK_UNIT: u32 = 2;
const RAMP_UNIT: u32 = 3;
const ENVIRONMENT_UNIT: u32 = 4;

pub(crate) struct ShineUniforms {
    enabled: i32,
    mask: i32,
    ramp: i32,
    eye: i32,
    tint: i32,
    unlit: i32,
    alpha_blend: i32,
    passes: i32,
    specular: i32,
    environment: i32,
    reflection: i32,
}
impl ShineUniforms {
    pub fn new(program: u32) -> Self {
        let loc = |name: &str| unsafe {
            gl::GetUniformLocation(program, std::ffi::CString::new(name).unwrap().as_ptr())
        };
        Self {
            enabled: loc("shineEnabled"),
            mask: loc("shineMask"),
            ramp: loc("shineRamp"),
            eye: loc("shineEye"),
            tint: loc("shineTint"),
            unlit: loc("shineUnlit"),
            alpha_blend: loc("shineAlphaBlend"),
            passes: loc("shinePasses"),
            specular: loc("shineSpecular"),
            environment: loc("shineEnvironment"),
            reflection: loc("shineReflection"),
        }
    }

    pub fn bind(
        &self,
        shine: Option<&Shine>,
        lights: &LightArray,
        context: &EngineRenderContext,
        view: &Matrix4<f32>,
    ) {
        unsafe {
            gl::Uniform1i(self.enabled, i32::from(shine.is_some()));
            gl::Uniform1i(self.mask, MASK_UNIT as i32);
            gl::Uniform1i(self.ramp, RAMP_UNIT as i32);
            gl::Uniform1i(self.environment, ENVIRONMENT_UNIT as i32);
            // Unsampled when disabled: the branch is uniform across the draw.
            let Some(shine) = shine else {
                return;
            };
            shine.mask.bind_to(context, MASK_UNIT);
            shine.ramp.bind_to(context, RAMP_UNIT);
            let eye = crate::scene::material::eye_position(view);
            gl::Uniform3fv(self.eye, 1, eye.as_ptr());
            gl::Uniform3fv(self.tint, 1, shine.tint.as_ptr());
            gl::Uniform1i(self.unlit, i32::from(shine.unlit));
            gl::Uniform1i(
                self.alpha_blend,
                i32::from(shine.blend == ShineBlend::Alpha),
            );
            gl::Uniform1f(self.passes, shine.passes as f32);
            gl::Uniform1f(self.specular, lights.specular * shine.specular);
            let reflection = match &lights.environment {
                Some(environment) => {
                    environment.bind_to(ENVIRONMENT_UNIT);
                    lights.reflection
                }
                None => 0.0,
            };
            gl::Uniform1f(self.reflection, reflection);
        }
    }
}
