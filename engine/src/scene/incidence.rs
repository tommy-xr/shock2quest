//! An optional view-angle lookup for an additive material pass. Shared by
//! static and skinned shaders so the ramp has one interpretation.
use crate::{engine::EngineRenderContext, texture::TextureTrait};
use cgmath::{Matrix4, Vector3, prelude::*};
use std::rc::Rc;

#[derive(Clone)]
pub struct IncidencePass {
    pub ramp: Rc<dyn TextureTrait>,
    pub tint: Vector3<f32>,
    pub unlit: bool,
    /// Flat decals can carry vertex normals unrelated to their authored face.
    /// Derivatives recover that face without changing the base material.
    pub geometric_normal: bool,
}

pub(crate) const GLSL: &str = r#"
uniform bool incidenceEnabled;
uniform sampler2D incidenceRamp;
uniform vec3 incidenceEye;
uniform vec3 incidenceTint;
uniform bool incidenceUnlit;
uniform bool incidenceGeometricNormal;
vec4 applyIncidence(vec4 lit, vec4 texel, vec3 position, vec3 normal) {
    if (!incidenceEnabled) return lit;
    vec3 surfaceNormal = incidenceGeometricNormal ? cross(dFdx(position), dFdy(position)) : normal;
    float facing = clamp(abs(dot(normalize(surfaceNormal), normalize(incidenceEye - position))), 0.0, 1.0);
    float alpha = texture(incidenceRamp, vec2(facing, 0.5)).r;
    return vec4((incidenceUnlit ? texel.rgb : lit.rgb) * incidenceTint, lit.a * alpha);
}
"#;

pub(crate) struct IncidenceUniforms {
    enabled: i32,
    ramp: i32,
    eye: i32,
    tint: i32,
    unlit: i32,
    geometric_normal: i32,
}
impl IncidenceUniforms {
    pub fn new(program: u32) -> Self {
        let loc = |name: &str| unsafe {
            gl::GetUniformLocation(program, std::ffi::CString::new(name).unwrap().as_ptr())
        };
        Self {
            enabled: loc("incidenceEnabled"),
            ramp: loc("incidenceRamp"),
            eye: loc("incidenceEye"),
            tint: loc("incidenceTint"),
            unlit: loc("incidenceUnlit"),
            geometric_normal: loc("incidenceGeometricNormal"),
        }
    }
    pub fn bind(
        &self,
        pass: Option<&IncidencePass>,
        context: &EngineRenderContext,
        view: &Matrix4<f32>,
    ) {
        unsafe {
            gl::Uniform1i(self.enabled, i32::from(pass.is_some()));
            gl::Uniform1i(self.ramp, 1);
            if let Some(pass) = pass {
                pass.ramp.bind1(context);
                let eye = view.invert().unwrap_or_else(Matrix4::identity).w.truncate();
                gl::Uniform3fv(self.eye, 1, eye.as_ptr());
                gl::Uniform3fv(self.tint, 1, pass.tint.as_ptr());
                gl::Uniform1i(self.unlit, i32::from(pass.unlit));
                gl::Uniform1i(self.geometric_normal, i32::from(pass.geometric_normal));
            }
        }
    }
}
