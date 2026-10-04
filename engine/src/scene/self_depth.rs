//! Private self-occlusion for translucent actors. Nothing here writes scene depth.
use super::{RenderLayer, scene::Scene};
use crate::{engine::EngineRenderContext, gl_engine::OpenGLEngine};
use cgmath::Matrix4;

/// Explicit phases prevent the scratch pass from sampling its own attachment.
#[derive(Clone, Copy, Default)]
pub enum Phase {
    #[default]
    None,
    Capture,
    Test(Mask),
}

/// Bound only for the actor whose nearest surface is in the scratch texture.
#[derive(Clone, Copy)]
pub struct Mask {
    texture: u32,
    origin: [i32; 2],
}

// Units 0–1 hold diffuse/pass textures, 2–5 shine maps, and 7 the environment cube.
const TEXTURE_UNIT: u32 = 6;

pub(crate) const GLSL: &str = r#"
uniform bool selfDepthEnabled;
uniform highp sampler2D selfDepthTexture;
uniform ivec2 selfDepthOrigin;
bool hiddenBySelfDepth() {
    if (!selfDepthEnabled) return false;
    highp float nearest = texelFetch(selfDepthTexture, ivec2(gl_FragCoord.xy) - selfDepthOrigin, 0).r;
    // One depth24 quantization step, plus rounding between the two draws.
    return gl_FragCoord.z > nearest + 0.00000012;
}
"#;

/// Shares the material's vertex shader and texture sampling, but never executes
/// lighting or the color shader's control flow while capturing depth.
pub(crate) const FRAGMENT: &str = r#"
out vec4 fragColor;
in vec2 texCoord;
in vec3 worldPos;
in vec3 worldNormal;
uniform sampler2D texture1;
void main() {
    vec4 texel = sampleMaterialPass(texture1, texCoord, worldPos, worldNormal);
    if (materialAlphaRejected(texel.a) || texel.a <= 0.0
        || (materialPassEnabled && materialPassAlpha <= 0.0)) discard;
    fragColor = vec4(0.0);
}
"#;

pub(crate) struct Uniforms {
    enabled: i32,
    texture: i32,
    origin: i32,
}

impl Uniforms {
    pub fn new(program: u32) -> Self {
        unsafe {
            Self {
                enabled: gl::GetUniformLocation(program, c"selfDepthEnabled".as_ptr()),
                texture: gl::GetUniformLocation(program, c"selfDepthTexture".as_ptr()),
                origin: gl::GetUniformLocation(program, c"selfDepthOrigin".as_ptr()),
            }
        }
    }

    pub fn bind(&self, phase: Phase) {
        let mask = match phase {
            Phase::Test(mask) => Some(mask),
            _ => None,
        };
        unsafe {
            gl::Uniform1i(self.enabled, i32::from(mask.is_some()));
            // With no mask, point at diffuse unit 0, never at the texture being
            // rendered into. Even an untaken shader branch must avoid feedback.
            gl::Uniform1i(
                self.texture,
                if mask.is_some() {
                    TEXTURE_UNIT as i32
                } else {
                    0
                },
            );
            if let Some(mask) = mask {
                gl::ActiveTexture(gl::TEXTURE0 + TEXTURE_UNIT);
                gl::BindTexture(gl::TEXTURE_2D, mask.texture);
                gl::Uniform2i(self.origin, mask.origin[0], mask.origin[1]);
                gl::ActiveTexture(gl::TEXTURE0);
            }
        }
    }
}

/// Lazily allocated, reused across actors/eyes, resized with the active viewport.
#[derive(Default)]
pub(crate) struct Target {
    framebuffer: u32,
    texture: u32,
    size: [i32; 2],
}

impl Target {
    pub fn render(
        &mut self,
        group: u64,
        layer: RenderLayer,
        engine: &OpenGLEngine,
        context: &EngineRenderContext,
        view: &Matrix4<f32>,
        scene: &Scene,
    ) -> Mask {
        unsafe {
            let mut viewport = [0; 4];
            let mut draw_framebuffer = 0;
            let mut read_framebuffer = 0;
            let mut color_mask = [0; 4];
            let mut depth_func = 0;
            let mut depth_mask = 0;
            gl::GetIntegerv(gl::VIEWPORT, viewport.as_mut_ptr());
            gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut draw_framebuffer);
            gl::GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut read_framebuffer);
            gl::GetIntegerv(gl::DEPTH_FUNC, &mut depth_func);
            gl::GetBooleanv(gl::COLOR_WRITEMASK, color_mask.as_mut_ptr());
            gl::GetBooleanv(gl::DEPTH_WRITEMASK, &mut depth_mask);
            let scissor = gl::IsEnabled(gl::SCISSOR_TEST);
            gl::Disable(gl::SCISSOR_TEST);
            gl::ActiveTexture(gl::TEXTURE0 + TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_2D, 0);

            if self.framebuffer == 0 {
                gl::GenFramebuffers(1, &mut self.framebuffer);
                gl::GenTextures(1, &mut self.texture);
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffer);
            let size = [viewport[2], viewport[3]];
            if self.size != size {
                gl::BindTexture(gl::TEXTURE_2D, self.texture);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::DEPTH_COMPONENT24 as i32,
                    size[0],
                    size[1],
                    0,
                    gl::DEPTH_COMPONENT,
                    gl::UNSIGNED_INT,
                    std::ptr::null(),
                );
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    gl::DEPTH_ATTACHMENT,
                    gl::TEXTURE_2D,
                    self.texture,
                    0,
                );
                gl::DrawBuffers(1, &gl::NONE);
                gl::ReadBuffer(gl::NONE);
                assert_eq!(
                    gl::CheckFramebufferStatus(gl::FRAMEBUFFER),
                    gl::FRAMEBUFFER_COMPLETE,
                    "apparition self-depth framebuffer is incomplete"
                );
                gl::BindTexture(gl::TEXTURE_2D, 0);
                self.size = size;
            }
            gl::ActiveTexture(gl::TEXTURE0);
            gl::Viewport(0, 0, size[0], size[1]);
            gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
            gl::DepthMask(gl::TRUE);
            gl::DepthFunc(gl::LESS);
            gl::Clear(gl::DEPTH_BUFFER_BIT);

            let context = EngineRenderContext {
                self_depth: Phase::Capture,
                ..*context
            };
            for object in scene
                .objects_in_layer(layer)
                .filter(|object| object.self_depth_group == Some(group))
            {
                if let Some(depth) = object.self_depth_object() {
                    depth.draw_opaque(engine, &context, view, scene.lights());
                }
            }

            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, draw_framebuffer as u32);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, read_framebuffer as u32);
            gl::Viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            gl::ColorMask(color_mask[0], color_mask[1], color_mask[2], color_mask[3]);
            gl::DepthMask(depth_mask);
            gl::DepthFunc(depth_func as u32);
            if scissor == gl::TRUE {
                gl::Enable(gl::SCISSOR_TEST);
            }
            Mask {
                texture: self.texture,
                origin: [viewport[0], viewport[1]],
            }
        }
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        if self.framebuffer == 0 {
            return;
        }
        unsafe {
            gl::DeleteFramebuffers(1, &self.framebuffer);
            gl::DeleteTextures(1, &self.texture);
        }
    }
}
