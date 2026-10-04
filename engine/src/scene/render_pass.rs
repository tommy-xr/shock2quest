//! Authored material-pass state shared by rigid and skinned model shaders.
use crate::{
    engine::EngineRenderContext,
    texture::{CubeTexture, TextureTrait},
};
use cgmath::{Matrix4, Vector3, prelude::*};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcAlpha,
    InvSrcAlpha,
    SrcColor,
    InvSrcColor,
    DstAlpha,
    InvDstAlpha,
    DstColor,
    InvDstColor,
}
impl BlendFactor {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_uppercase().as_str() {
            "ZERO" => Self::Zero,
            "ONE" => Self::One,
            "SRC_ALPHA" => Self::SrcAlpha,
            "INV_SRC_ALPHA" => Self::InvSrcAlpha,
            "SRC_COLOR" => Self::SrcColor,
            "INV_SRC_COLOR" => Self::InvSrcColor,
            "DST_ALPHA" => Self::DstAlpha,
            "INV_DST_ALPHA" => Self::InvDstAlpha,
            "DST_COLOR" => Self::DstColor,
            "INV_DST_COLOR" => Self::InvDstColor,
            _ => return None,
        })
    }
    pub(crate) fn gl(self) -> u32 {
        match self {
            Self::Zero => gl::ZERO,
            Self::One => gl::ONE,
            Self::SrcAlpha => gl::SRC_ALPHA,
            Self::InvSrcAlpha => gl::ONE_MINUS_SRC_ALPHA,
            Self::SrcColor => gl::SRC_COLOR,
            Self::InvSrcColor => gl::ONE_MINUS_SRC_COLOR,
            Self::DstAlpha => gl::DST_ALPHA,
            Self::InvDstAlpha => gl::ONE_MINUS_DST_ALPHA,
            Self::DstColor => gl::DST_COLOR,
            Self::InvDstColor => gl::ONE_MINUS_DST_COLOR,
        }
    }
}

#[derive(Clone)]
pub struct RenderPass {
    pub color: Vector3<f32>,
    pub alpha: f32,
    pub replace_alpha: bool,
    pub force_opaque: bool,
    pub alpha_test: bool,
    pub shaded: bool,
    pub mipmap_bias: f32,
    pub incidence: Option<(Rc<dyn TextureTrait>, f32)>,
    pub cube: Option<Rc<CubeTexture>>,
}
impl RenderPass {
    /// NewDark replaces the surface's vertex alpha, never the bitmap's alpha.
    pub fn vertex_alpha(&self, opacity: f32) -> f32 {
        self.alpha * if self.replace_alpha { 1.0 } else { opacity }
    }
}
impl Default for RenderPass {
    fn default() -> Self {
        Self {
            color: Vector3::new(1.0, 1.0, 1.0),
            alpha: 1.0,
            replace_alpha: false,
            force_opaque: false,
            alpha_test: false,
            shaded: false,
            mipmap_bias: 0.0,
            incidence: None,
            cube: None,
        }
    }
}

pub(crate) const GLSL: &str = r#"
uniform bool materialPassEnabled;
uniform vec3 materialPassColor;
uniform float materialPassAlpha;
uniform bool materialPassForceOpaque;
uniform bool materialPassAlphaTest;
uniform bool materialPassShaded;
uniform float materialPassMipmapBias;
uniform bool materialPassIncidence;
uniform sampler2D materialPassRamp;
uniform float materialPassMaxDistance;
uniform vec3 materialPassEye;
uniform bool materialPassCubeEnabled;
uniform samplerCube materialPassCube;
bool materialAlphaRejected(float alpha) {
    return (!materialPassEnabled || materialPassAlphaTest) && alpha < 0.1;
}
vec4 sampleMaterialPass(sampler2D tex, vec2 uv, vec3 position, vec3 normal) {
    vec4 pixel;
    if (materialPassEnabled && materialPassCubeEnabled) {
        vec3 ray = reflect(materialPassEye - position, normalize(normal));
        pixel = texture(materialPassCube, environmentDirection(ray), materialPassMipmapBias);
    } else {
        pixel = texture(tex, uv, materialPassEnabled ? materialPassMipmapBias : 0.0);
    }
    if (materialPassEnabled && materialPassForceOpaque) pixel.a = 1.0;
    return pixel;
}
vec4 applyMaterialPass(vec4 lit, vec4 texel, vec3 position, vec3 normal) {
    if (!materialPassEnabled) return lit;
    float alpha = materialPassAlpha;
    if (materialPassIncidence) {
        vec3 ray = materialPassEye - position;
        // 25AE's shipped DarkMaterial.inc uses inverse incidence angle and red LUT channel.
        float angle = acos(clamp(dot(normalize(normal), normalize(ray)), -1.0, 1.0));
        vec2 uv = vec2(1.0 - angle / 1.57079632679, length(ray) / materialPassMaxDistance);
        alpha *= texture(materialPassRamp, clamp(uv, 0.0, 1.0)).r;
    }
    return vec4((materialPassShaded && !materialPassCubeEnabled ? lit.rgb : texel.rgb) * materialPassColor,
        texel.a * alpha);
}
"#;

pub(crate) struct Uniforms {
    enabled: i32,
    color: i32,
    alpha: i32,
    force_opaque: i32,
    alpha_test: i32,
    shaded: i32,
    mipmap_bias: i32,
    incidence: i32,
    ramp: i32,
    max_distance: i32,
    eye: i32,
    cube_enabled: i32,
    cube: i32,
}
impl Uniforms {
    pub fn new(program: u32) -> Self {
        let loc = |name: &str| unsafe {
            gl::GetUniformLocation(program, std::ffi::CString::new(name).unwrap().as_ptr())
        };
        Self {
            enabled: loc("materialPassEnabled"),
            color: loc("materialPassColor"),
            alpha: loc("materialPassAlpha"),
            force_opaque: loc("materialPassForceOpaque"),
            alpha_test: loc("materialPassAlphaTest"),
            shaded: loc("materialPassShaded"),
            mipmap_bias: loc("materialPassMipmapBias"),
            incidence: loc("materialPassIncidence"),
            ramp: loc("materialPassRamp"),
            max_distance: loc("materialPassMaxDistance"),
            eye: loc("materialPassEye"),
            cube_enabled: loc("materialPassCubeEnabled"),
            cube: loc("materialPassCube"),
        }
    }
    pub fn bind(
        &self,
        pass: Option<&RenderPass>,
        context: &EngineRenderContext,
        view: &Matrix4<f32>,
        opacity: f32,
    ) {
        unsafe {
            gl::Uniform1i(self.enabled, i32::from(pass.is_some()));
            // Sampler types must use distinct units even when their branch is disabled.
            gl::Uniform1i(self.ramp, 1);
            gl::Uniform1i(self.cube, 7);
            if let Some(p) = pass {
                gl::Uniform3fv(self.color, 1, p.color.as_ptr());
                gl::Uniform1f(self.alpha, p.vertex_alpha(opacity));
                gl::Uniform1i(self.force_opaque, i32::from(p.force_opaque));
                gl::Uniform1i(self.alpha_test, i32::from(p.alpha_test));
                gl::Uniform1i(self.shaded, i32::from(p.shaded));
                gl::Uniform1f(self.mipmap_bias, p.mipmap_bias);
                gl::Uniform1i(self.incidence, i32::from(p.incidence.is_some()));
                if let Some((ramp, distance)) = &p.incidence {
                    ramp.bind1(context);
                    gl::Uniform1f(self.max_distance, *distance);
                }
                let eye = view.invert().unwrap_or_else(Matrix4::identity).w.truncate();
                gl::Uniform3fv(self.eye, 1, eye.as_ptr());
                gl::Uniform1i(self.cube_enabled, i32::from(p.cube.is_some()));
                if let Some(cube) = &p.cube {
                    cube.bind_to(7);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_alpha_ignores_entity_fade_but_multiplication_does_not() {
        let mut pass = RenderPass {
            alpha: 0.2,
            ..Default::default()
        };
        assert_eq!(pass.vertex_alpha(0.0), 0.0);
        assert_eq!(pass.vertex_alpha(0.5), 0.1);
        pass.replace_alpha = true;
        assert_eq!(pass.vertex_alpha(0.0), 0.2);
        assert_eq!(pass.vertex_alpha(0.5), 0.2);
    }
}
