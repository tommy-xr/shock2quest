//! An offscreen color + depth target that a scene pass renders into and a
//! material later samples. GL objects are created on first bind, on the GL thread.
use crate::scene::pass_focus::Sampling;
use std::cell::Cell;

/// Portal views are distorted on display, so a small target is enough.
pub const DEFAULT_SIZE: [i32; 2] = [256, 256];

/// Units 0–1 hold diffuse/pass textures, 2–5 shine maps, 6 self-depth and 7 the
/// environment cube. Sampling a target always goes through this unit, so a
/// pass only has to unbind one unit to avoid a feedback loop.
pub const TEXTURE_UNIT: u32 = 8;

#[derive(Default, Clone, Copy)]
struct Handles {
    fbo: u32,
    color_texture: u32,
    depth_renderbuffer: u32,
}

#[derive(Clone, Copy)]
struct Saved {
    framebuffer: i32,
    viewport: [i32; 4],
    scissor: bool,
}

pub struct RenderTarget {
    size: [i32; 2],
    handles: Cell<Handles>,
    saved: Cell<Option<Saved>>,
    sampling: Cell<Option<Sampling>>,
}

impl RenderTarget {
    pub fn new(size: [i32; 2]) -> Self {
        Self {
            size,
            handles: Cell::new(Handles::default()),
            saved: Cell::new(None),
            sampling: Cell::new(None),
        }
    }

    pub fn size(&self) -> [i32; 2] {
        self.size
    }

    /// Where the last render put each world point, or `None` before one.
    pub fn sampling(&self) -> Option<Sampling> {
        self.sampling.get()
    }

    pub fn set_sampling(&self, sampling: Sampling) {
        self.sampling.set(Some(sampling));
    }

    /// The color attachment, or 0 before the first `bind`.
    pub fn color_texture(&self) -> u32 {
        self.handles.get().color_texture
    }

    /// Redirect drawing into this target, remembering the caller's framebuffer,
    /// viewport and scissor for `unbind`.
    pub fn bind(&self) {
        unsafe {
            let mut framebuffer = 0;
            let mut viewport = [0; 4];
            gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut framebuffer);
            gl::GetIntegerv(gl::VIEWPORT, viewport.as_mut_ptr());
            let scissor = gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;
            self.saved.set(Some(Saved {
                framebuffer,
                viewport,
                scissor,
            }));

            // The previous frame may have left this texture bound for sampling.
            gl::ActiveTexture(gl::TEXTURE0 + TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            gl::ActiveTexture(gl::TEXTURE0);

            let handles = self.ensure_created();
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, handles.fbo);
            gl::Disable(gl::SCISSOR_TEST);
            gl::Viewport(0, 0, self.size[0], self.size[1]);
        }
    }

    /// Restore what `bind` saved.
    pub fn unbind(&self) {
        let Some(saved) = self.saved.take() else {
            return;
        };
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, saved.framebuffer as u32);
            gl::Viewport(
                saved.viewport[0],
                saved.viewport[1],
                saved.viewport[2],
                saved.viewport[3],
            );
            if saved.scissor {
                gl::Enable(gl::SCISSOR_TEST);
            }
        }
    }

    unsafe fn ensure_created(&self) -> Handles {
        let mut handles = self.handles.get();
        if handles.fbo != 0 {
            return handles;
        }
        unsafe {
            gl::GenFramebuffers(1, &mut handles.fbo);
            gl::GenTextures(1, &mut handles.color_texture);
            gl::GenRenderbuffers(1, &mut handles.depth_renderbuffer);

            gl::BindTexture(gl::TEXTURE_2D, handles.color_texture);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                self.size[0],
                self.size[1],
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::BindTexture(gl::TEXTURE_2D, 0);

            gl::BindRenderbuffer(gl::RENDERBUFFER, handles.depth_renderbuffer);
            gl::RenderbufferStorage(
                gl::RENDERBUFFER,
                gl::DEPTH_COMPONENT24,
                self.size[0],
                self.size[1],
            );
            gl::BindRenderbuffer(gl::RENDERBUFFER, 0);

            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, handles.fbo);
            gl::FramebufferTexture2D(
                gl::DRAW_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                handles.color_texture,
                0,
            );
            gl::FramebufferRenderbuffer(
                gl::DRAW_FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::RENDERBUFFER,
                handles.depth_renderbuffer,
            );
            assert_eq!(
                gl::CheckFramebufferStatus(gl::DRAW_FRAMEBUFFER),
                gl::FRAMEBUFFER_COMPLETE,
                "offscreen render target is incomplete"
            );
        }
        self.handles.set(handles);
        handles
    }
}

impl Default for RenderTarget {
    fn default() -> Self {
        Self::new(DEFAULT_SIZE)
    }
}

impl Drop for RenderTarget {
    fn drop(&mut self) {
        let handles = self.handles.get();
        if handles.fbo == 0 {
            return;
        }
        unsafe {
            gl::DeleteFramebuffers(1, &handles.fbo);
            gl::DeleteTextures(1, &handles.color_texture);
            gl::DeleteRenderbuffers(1, &handles.depth_renderbuffer);
        }
    }
}
