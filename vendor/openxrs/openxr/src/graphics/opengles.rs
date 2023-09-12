use std::ptr;

use sys::platform::*;

use crate::*;

/// The OpenGL ES graphics API
///
/// See [`XR_KHR_opengl_es_enable`] for safety details.
///
/// [`XR_KHR_opengl_es_enable`]: https://www.khronos.org/registry/OpenXR/specs/1.0/html/xrspec.html#XR_KHR_opengl_enable
pub enum OpenGLES {}

impl Graphics for OpenGLES {
    type Requirements = Requirements;
    type SessionCreateInfo = SessionCreateInfo;
    type Format = u32;
    type SwapchainImage = u32;

    fn raise_format(x: i64) -> u32 {
        x as _
    }
    fn lower_format(x: u32) -> i64 {
        x.into()
    }

    fn requirements(inst: &Instance, system: SystemId) -> Result<Requirements> {
        let out = unsafe {
            let mut x = sys::GraphicsRequirementsOpenGLESKHR::out(ptr::null_mut());
            cvt((inst.opengles().get_open_gles_graphics_requirements)(
                inst.as_raw(),
                system,
                x.as_mut_ptr(),
            ))?;
            x.assume_init()
        };
        Ok(Requirements {
            min_api_version_supported: out.min_api_version_supported,
            max_api_version_supported: out.max_api_version_supported,
        })
    }

    unsafe fn create_session(
        instance: &Instance,
        system: SystemId,
        info: &Self::SessionCreateInfo,
    ) -> Result<sys::Session> {
        match *info {
            #[cfg(target_os = "android")]
            SessionCreateInfo::Android {
                display,
                config,
                context,
            } => {
                let binding = sys::GraphicsBindingOpenGLESAndroidKHR {
                    ty: sys::GraphicsBindingOpenGLESAndroidKHR::TYPE,
                    next: ptr::null(),
                    display,
                    config,
                    context,
                };
                let info = sys::SessionCreateInfo {
                    ty: sys::SessionCreateInfo::TYPE,
                    next: &binding as *const _ as *const _,
                    create_flags: Default::default(),
                    system_id: system,
                };
                let mut out = sys::Session::NULL;
                cvt((instance.fp().create_session)(
                    instance.as_raw(),
                    &info,
                    &mut out,
                ))?;
                Ok(out)
            }

            #[cfg(windows)]
            SessionCreateInfo::Windows { h_dc, h_glrc } => {
                let binding = sys::GraphicsBindingOpenGLESWin32KHR {
                    ty: sys::GraphicsBindingOpenGLESWin32KHR::TYPE,
                    next: ptr::null(),
                    h_dc,
                    h_glrc,
                };
                let info = sys::SessionCreateInfo {
                    ty: sys::SessionCreateInfo::TYPE,
                    next: &binding as *const _ as *const _,
                    create_flags: Default::default(),
                    system_id: system,
                };
                let mut out = sys::Session::NULL;
                cvt((instance.fp().create_session)(
                    instance.as_raw(),
                    &info,
                    &mut out,
                ))?;
                Ok(out)
            }
            SessionCreateInfo::Xlib {
                x_display,
                visualid,
                glx_fb_config,
                glx_drawable,
                glx_context,
            } => {
                let binding = sys::GraphicsBindingOpenGLXlibKHR {
                    ty: sys::GraphicsBindingOpenGLXlibKHR::TYPE,
                    next: ptr::null(),
                    x_display,
                    visualid,
                    glx_fb_config,
                    glx_drawable,
                    glx_context,
                };
                let info = sys::SessionCreateInfo {
                    ty: sys::SessionCreateInfo::TYPE,
                    next: &binding as *const _ as *const _,
                    create_flags: Default::default(),
                    system_id: system,
                };
                let mut out = sys::Session::NULL;
                cvt((instance.fp().create_session)(
                    instance.as_raw(),
                    &info,
                    &mut out,
                ))?;
                Ok(out)
            }
        }
    }

    fn enumerate_swapchain_images(
        swapchain: &Swapchain<Self>,
    ) -> Result<Vec<Self::SwapchainImage>> {
        let images = get_arr_init(
            sys::SwapchainImageOpenGLESKHR {
                ty: sys::SwapchainImageOpenGLESKHR::TYPE,
                next: ptr::null_mut(),
                image: 0,
            },
            |capacity, count, buf| unsafe {
                (swapchain.instance().fp().enumerate_swapchain_images)(
                    swapchain.as_raw(),
                    capacity,
                    count,
                    buf as *mut _,
                )
            },
        )?;
        Ok(images.into_iter().map(|x| x.image).collect())
    }
}

#[derive(Copy, Clone)]
pub struct Requirements {
    pub min_api_version_supported: Version,
    pub max_api_version_supported: Version,
}

pub enum SessionCreateInfo {
    Xlib {
        x_display: *mut Display,
        visualid: u32,
        glx_fb_config: GLXFBConfig,
        glx_drawable: GLXDrawable,
        glx_context: GLXContext,
    },
    #[cfg(windows)]
    Windows { h_dc: HDC, h_glrc: HGLRC },
    #[cfg(target_os = "android")]
    Android {
        display: *mut Display,
        config: GLXFBConfig,
        context: GLXContext,
    },
}
