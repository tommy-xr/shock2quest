//! Authored view-angle shine drawn in the base pass. A 25AE material stacks
//! identical `INCIDENCE` passes over its diffuse; composing them in the same
//! fragment gives the stacked result without re-drawing (and re-skinning) the
//! mesh once per pass. Shared by static and skinned shaders.
use crate::{engine::EngineRenderContext, texture::TextureTrait};
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
vec4 applyShine(vec4 base, vec3 light, float opacity, vec2 uv, vec3 position, vec3 normal) {
    if (!shineEnabled) return base;
    vec4 mask = texture(shineMask, uv);
    // A separate pass would alpha-test its own bitmap.
    if (mask.a < 0.1) return base;
    float facing = clamp(abs(dot(normal, normalize(shineEye - position))), 0.0, 1.0);
    float alpha = mask.a * opacity * texture(shineRamp, vec2(facing, 0.5)).r;
    vec3 shine = min((shineUnlit ? mask.rgb : mask.rgb * light) * shineTint, vec3(1.0));
    // Each stacked pass blended onto a clamped framebuffer.
    vec3 color = min(base.rgb, vec3(1.0));
    float coverage = max(base.a, 1.0 / 255.0);
    if (shineAlphaBlend) {
        float covered = 1.0 - pow(1.0 - alpha, shinePasses);
        float outAlpha = coverage * (1.0 - covered) + covered;
        return vec4((color * coverage * (1.0 - covered) + shine * covered) / outAlpha, outAlpha);
    }
    // The blend scales colour by the base's coverage; an additive pass does
    // not. Exact until a faded base's pre-divided colour saturates.
    return vec4(color + shinePasses * shine * alpha / coverage, base.a);
}
"#;

const MASK_UNIT: u32 = 2;
const RAMP_UNIT: u32 = 3;

pub(crate) struct ShineUniforms {
    enabled: i32,
    mask: i32,
    ramp: i32,
    eye: i32,
    tint: i32,
    unlit: i32,
    alpha_blend: i32,
    passes: i32,
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
        }
    }

    pub fn bind(&self, shine: Option<&Shine>, context: &EngineRenderContext, view: &Matrix4<f32>) {
        unsafe {
            gl::Uniform1i(self.enabled, i32::from(shine.is_some()));
            gl::Uniform1i(self.mask, MASK_UNIT as i32);
            gl::Uniform1i(self.ramp, RAMP_UNIT as i32);
            // Unsampled when disabled: the branch is uniform across the draw.
            let Some(shine) = shine else {
                return;
            };
            shine.mask.bind_to(context, MASK_UNIT);
            shine.ramp.bind_to(context, RAMP_UNIT);
            let eye = view.invert().unwrap_or_else(Matrix4::identity).w.truncate();
            gl::Uniform3fv(self.eye, 1, eye.as_ptr());
            gl::Uniform3fv(self.tint, 1, shine.tint.as_ptr());
            gl::Uniform1i(self.unlit, i32::from(shine.unlit));
            gl::Uniform1i(
                self.alpha_blend,
                i32::from(shine.blend == ShineBlend::Alpha),
            );
            gl::Uniform1f(self.passes, shine.passes as f32);
        }
    }
}
