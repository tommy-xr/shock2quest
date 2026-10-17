//! Opt-in OVR_multiview2 stereo: both eyes render in one pass, each draw
//! submitted once and its vertex shader run per eye layer. Process-wide,
//! because shader programs are compiled once and shared: enable it before the
//! first draw. Off (the default) leaves shaders and uniforms exactly as before.
use cgmath::{Array, Matrix, Matrix4, SquareMatrix, vec4};
use gl::types::{GLenum, GLint, GLsizei, GLuint};
use std::sync::OnceLock;

pub const VIEW_COUNT: usize = 2;

type AttachFn = unsafe extern "system" fn(GLenum, GLenum, GLuint, GLint, GLint, GLsizei);
static ATTACH: OnceLock<AttachFn> = OnceLock::new();

/// Enable multiview for the process.
///
/// # Safety
/// `framebuffer_texture_multiview_ovr` must be the GL context's
/// `glFramebufferTextureMultiviewOVR`.
pub unsafe fn enable(framebuffer_texture_multiview_ovr: *const std::ffi::c_void) {
    assert!(!framebuffer_texture_multiview_ovr.is_null());
    let attach = unsafe {
        std::mem::transmute::<*const std::ffi::c_void, AttachFn>(framebuffer_texture_multiview_ovr)
    };
    let _ = ATTACH.set(attach);
}

pub fn is_enabled() -> bool {
    ATTACH.get().is_some()
}

/// Attach every layer of a 2-layer array texture to the bound framebuffer.
///
/// # Safety
/// Requires `enable`, a current GL context and a bound framebuffer.
pub unsafe fn attach_layers(target: GLenum, attachment: GLenum, texture: GLuint) {
    let attach = ATTACH.get().expect("multiview is not enabled");
    unsafe { attach(target, attachment, texture, 0, 0, VIEW_COUNT as GLsizei) };
}

/// Per-eye matrices for one multiview pass. Materials still receive eye 0's
/// view; `eye_from_view` carries it into each eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Views {
    pub eye_from_view: [Matrix4<f32>; VIEW_COUNT],
    pub projection: [Matrix4<f32>; VIEW_COUNT],
    /// An object built for one eye (`render_per_eye`) draws only in that eye.
    pub only_eye: Option<usize>,
}

/// Sends every vertex outside the clip volume, so the draw rasterizes nothing.
pub const CULL: Matrix4<f32> = Matrix4::from_cols(
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(0.0, 0.0, 0.0, 0.0),
    vec4(2.0, 2.0, 2.0, 1.0),
);

impl Views {
    /// Built from each eye's full view; eye 0's is the view materials receive.
    pub fn new(view: [Matrix4<f32>; VIEW_COUNT], projection: [Matrix4<f32>; VIEW_COUNT]) -> Self {
        // In f64: view translations are hundreds of units.
        let base = view[0]
            .cast::<f64>()
            .unwrap()
            .invert()
            .expect("view is invertible");
        Views {
            eye_from_view: view.map(|v| (v.cast::<f64>().unwrap() * base).cast().unwrap()),
            projection,
            only_eye: None,
        }
    }

    /// The `view` / `projection` uniform arrays for one draw. A camera draw
    /// takes each eye's own matrices; a screen-space draw keeps its own
    /// (eye-independent) pair in both eyes.
    pub fn uniforms(
        &self,
        view: &Matrix4<f32>,
        projection: &Matrix4<f32>,
        camera: bool,
    ) -> ([Matrix4<f32>; VIEW_COUNT], [Matrix4<f32>; VIEW_COUNT]) {
        let mut views = [*view; VIEW_COUNT];
        let mut projections = [*projection; VIEW_COUNT];
        for eye in 0..VIEW_COUNT {
            if camera {
                views[eye] = self.eye_from_view[eye] * view;
                projections[eye] = self.projection[eye];
            }
            if self.only_eye.is_some_and(|only| only != eye) {
                projections[eye] = CULL;
            }
        }
        (views, projections)
    }
}

/// Upload a draw's `view` / `projection` uniforms; a location of -1 skips it.
/// Single-view this is exactly the two plain uploads.
///
/// # Safety
/// Requires a current GL context with the draw's program in use.
pub(crate) unsafe fn bind_view_projection(
    view_loc: GLint,
    projection_loc: GLint,
    view: &Matrix4<f32>,
    projection: &Matrix4<f32>,
    context: &crate::EngineRenderContext,
    camera: bool,
) {
    unsafe {
        match context.multiview {
            None => {
                gl::UniformMatrix4fv(view_loc, 1, gl::FALSE, view.as_ptr());
                gl::UniformMatrix4fv(projection_loc, 1, gl::FALSE, projection.as_ptr());
            }
            Some(views) => {
                let (v, p) = views.uniforms(view, projection, camera);
                gl::UniformMatrix4fv(
                    view_loc,
                    VIEW_COUNT as GLsizei,
                    gl::FALSE,
                    v.as_ptr().cast(),
                );
                gl::UniformMatrix4fv(
                    projection_loc,
                    VIEW_COUNT as GLsizei,
                    gl::FALSE,
                    p.as_ptr().cast(),
                );
            }
        }
    }
}

/// Multiview declarations for one shader, inserted right after `#version`
/// (an `#extension` must precede every non-preprocessor token).
pub(crate) fn shader_header(vertex: bool) -> &'static str {
    if vertex {
        "#extension GL_OVR_multiview2 : require\n#define MULTIVIEW 1\nlayout(num_views = 2) in;"
    } else {
        "#extension GL_OVR_multiview2 : require\n#define MULTIVIEW 1"
    }
}

/// Uniforms that differ per eye: the camera matrices, and the eye position
/// fragment lighting reads (`eye` is the environment preview's).
const PER_EYE: [(&str, &str); 7] = [
    ("mat4", "view"),
    ("mat4", "projection"),
    ("vec3", "shineEye"),
    ("vec3", "incidenceEye"),
    ("vec3", "materialPassEye"),
    ("vec3", "eyePosition"),
    ("vec3", "eye"),
];

/// Turn each `PER_EYE` declaration (`uniform [precision] type name;`) into a
/// per-eye array indexed by `gl_ViewID_OVR`, in both stages (the vignette's
/// fragment shader reads `projection`, and linked declarations must match).
/// Bodies and uniform location names (`view` = `view[0]`) stay unchanged: a
/// macro is not re-expanded inside its own replacement, so `view` maps to
/// `view[gl_ViewID_OVR]` once.
pub(crate) fn rewrite_view_uniforms(shader: &str) -> String {
    let mut out = String::with_capacity(shader.len() + 256);
    for line in shader.lines() {
        let decl = line.trim();
        match per_eye_name(decl) {
            Some(name) => out.push_str(&format!(
                "{}[{VIEW_COUNT}];\n#define {name} {name}[gl_ViewID_OVR]\n",
                decl.trim_end_matches(';').trim_end()
            )),
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

fn per_eye_name(decl: &str) -> Option<&str> {
    let tokens: Vec<&str> = decl.strip_suffix(';')?.split_whitespace().collect();
    let (ty, name) = match tokens.as_slice() {
        ["uniform", ty, name] | ["uniform", "highp" | "mediump" | "lowp", ty, name] => (*ty, *name),
        _ => return None,
    };
    PER_EYE
        .iter()
        .any(|&(t, n)| t == ty && n == name)
        .then_some(name)
}

/// Upload the eye position derived from `view` (each eye's under multiview).
///
/// # Safety
/// Requires a current GL context with the draw's program in use.
pub(crate) unsafe fn bind_eye(
    loc: GLint,
    view: &Matrix4<f32>,
    context: &crate::EngineRenderContext,
) {
    let eye = |v: &Matrix4<f32>| crate::scene::material::eye_position(v);
    unsafe {
        match context.multiview {
            None => gl::Uniform3fv(loc, 1, eye(view).as_ptr()),
            Some(views) => {
                let eyes = views.eye_from_view.map(|e| eye(&(e * view)));
                gl::Uniform3fv(loc, VIEW_COUNT as GLsizei, eyes.as_ptr().cast());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Vector4, vec3};

    #[test]
    fn rewrites_only_view_and_projection_declarations() {
        let src = "uniform highp mat4 world;\n  uniform highp mat4 view;\nuniform mat4 projection;\nvoid main() { gl_Position = projection * view * world[3]; }";
        let out = rewrite_view_uniforms(src);
        assert!(out.contains("uniform highp mat4 world;\n"));
        assert!(out.contains("uniform highp mat4 view[2];\n#define view view[gl_ViewID_OVR]\n"));
        assert!(out.contains(
            "uniform mat4 projection[2];\n#define projection projection[gl_ViewID_OVR]\n"
        ));
        assert!(out.contains("gl_Position = projection * view * world[3];"));
    }

    #[test]
    fn rewrites_fragment_eye_uniforms_but_not_lookalikes() {
        let out = rewrite_view_uniforms(
            "uniform vec3 shineEye;\nuniform vec3 shineTint;\nuniform float eye;",
        );
        assert!(
            out.contains("uniform vec3 shineEye[2];\n#define shineEye shineEye[gl_ViewID_OVR]\n")
        );
        assert!(out.contains("uniform vec3 shineTint;\n"));
        assert!(out.contains("uniform float eye;\n"));
    }

    #[test]
    fn a_screen_shader_without_view_rewrites_only_projection() {
        let out =
            rewrite_view_uniforms("uniform mat4 projection;\nvec4 p = projection * vec4(1.0);");
        assert!(!out.contains("view"));
        assert!(out.contains("projection[2]"));
    }

    fn views() -> Views {
        let left = Matrix4::from_translation(vec3(-100.03, -5.0, 20.0));
        let right = Matrix4::from_translation(vec3(-100.1, -5.0, 20.0));
        let pl = cgmath::perspective(cgmath::Deg(90.0), 1.0, 0.1, 100.0);
        let pr = cgmath::perspective(cgmath::Deg(80.0), 1.0, 0.1, 100.0);
        Views::new([left, right], [pl, pr])
    }

    #[test]
    fn camera_draws_get_each_eyes_view_and_projection() {
        let views = views();
        let left = Matrix4::from_translation(vec3(-100.03, -5.0, 20.0));
        let (v, p) = views.uniforms(&left, &Matrix4::identity(), true);
        let point = Vector4::new(3.0, 4.0, 5.0, 1.0);
        assert!(((v[0] * point) - (left * point)).magnitude2() < 1e-8);
        let right = Matrix4::from_translation(vec3(-100.1, -5.0, 20.0));
        assert!(((v[1] * point) - (right * point)).magnitude2() < 1e-8);
        assert_eq!(p, views.projection);
    }

    #[test]
    fn screen_draws_keep_their_own_matrices_in_both_eyes() {
        let ortho = cgmath::ortho(0.0, 640.0, 480.0, 0.0, 0.0, 1.0);
        let id = Matrix4::identity();
        assert_eq!(views().uniforms(&id, &ortho, false), ([id; 2], [ortho; 2]));
    }

    #[test]
    fn a_per_eye_object_is_culled_in_the_other_eye() {
        let views = Views {
            only_eye: Some(1),
            ..views()
        };
        let (_, p) = views.uniforms(&Matrix4::identity(), &Matrix4::identity(), true);
        assert_eq!(p[0], CULL);
        assert_eq!(p[1], views.projection[1]);
        let clip = CULL * Vector4::new(12.0, -3.0, 7.0, 1.0);
        assert_eq!(clip, Vector4::new(2.0, 2.0, 2.0, 1.0));
    }
}
