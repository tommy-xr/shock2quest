//! Fixed GLES foveation. Profiles live at least as long as their swapchains.
//! Neither this module nor its configuration changes simulation or refresh rate.
use openxr as xr;
use xr::sys::Handle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    Off,
    #[default]
    Low,
    Medium,
    High,
}

impl Level {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn xr_level(self) -> xr::FoveationLevelFB {
        match self {
            Self::Off => xr::FoveationLevelFB::NONE,
            Self::Low => xr::FoveationLevelFB::LOW,
            Self::Medium => xr::FoveationLevelFB::MEDIUM,
            Self::High => xr::FoveationLevelFB::HIGH,
        }
    }
}

pub fn configured_level() -> Level {
    let path = shock2vr::paths::data_root().join("ffr-level.txt");
    match std::fs::read_to_string(&path) {
        Ok(value) => Level::parse(&value).unwrap_or_else(|| {
            println!(
                "SHOCK2QUEST_FFR_CONFIG invalid={:?} fallback=off",
                value.trim()
            );
            Level::Off
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Level::default(),
        Err(error) => {
            println!("SHOCK2QUEST_FFR_CONFIG error={error} fallback=off");
            Level::Off
        }
    }
}

pub fn enable_extensions(available: &xr::ExtensionSet, enabled: &mut xr::ExtensionSet) {
    let supported = available.fb_foveation
        && available.fb_foveation_configuration
        && available.fb_swapchain_update_state
        && available.fb_swapchain_update_state_opengl_es;
    enabled.fb_foveation = supported;
    enabled.fb_foveation_configuration = supported;
    enabled.fb_swapchain_update_state = supported;
    enabled.fb_swapchain_update_state_opengl_es = supported;
}

pub struct EyeSwapchains {
    // Rust drops fields in declaration order: all handles before their profile.
    pub handles: Vec<xr::Swapchain<xr::OpenGlEs>>,
    pub profile: Option<xr::FoveationProfileFB>,
    pub effective: Level,
}

fn has_scaled_bin_support() -> bool {
    // Called only on the render thread after GLES initialization.
    unsafe {
        let mut count = 0;
        gl::GetIntegerv(gl::NUM_EXTENSIONS, &mut count);
        (0..count).any(|index| {
            let name = gl::GetStringi(gl::EXTENSIONS, index as u32);
            !name.is_null()
                && std::ffi::CStr::from_ptr(name.cast()).to_bytes() == b"GL_QCOM_texture_foveated"
        })
    }
}

/// Apply to the complete stereo pair before allocating application FBOs. A
/// failed eye never leaves the other eye foveated: drop both and retry off.
pub fn create(
    session: &xr::Session<xr::OpenGlEs>,
    views: &[xr::ViewConfigurationView],
    requested: Level,
) -> xr::Result<EyeSwapchains> {
    let supported = session.instance().exts().fb_foveation.is_some() && has_scaled_bin_support();
    if requested != Level::Off && supported {
        match create_pair(session, views, requested) {
            Ok(pair) => {
                println!(
                    "SHOCK2QUEST_FFR requested={} effective={} dynamic=false status=applied",
                    requested.as_str(),
                    pair.effective.as_str()
                );
                return Ok(pair);
            }
            Err(error) => println!("SHOCK2QUEST_FFR_FAILURE error={error:?} fallback=off"),
        }
    }
    let pair = create_pair(session, views, Level::Off)?;
    println!(
        "SHOCK2QUEST_FFR requested={} effective=off dynamic=false status={}",
        requested.as_str(),
        if requested == Level::Off {
            "disabled"
        } else if supported {
            "failed"
        } else {
            "unsupported"
        }
    );
    Ok(pair)
}

fn create_pair(
    session: &xr::Session<xr::OpenGlEs>,
    views: &[xr::ViewConfigurationView],
    level: Level,
) -> xr::Result<EyeSwapchains> {
    let profile = if level == Level::Off {
        None
    } else {
        Some(
            session.create_foveation_profile(Some(xr::FoveationLevelProfile {
                level: level.xr_level(),
                vertical_offset: 0.0,
                dynamic: xr::FoveationDynamicFB::DISABLED,
            }))?,
        )
    };
    let mut handles = Vec::with_capacity(views.len());
    for view in views {
        let info = xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED,
            format: gl::SRGB8_ALPHA8,
            sample_count: 1,
            width: view.recommended_image_rect_width,
            height: view.recommended_image_rect_height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        };
        let handle = if let Some(profile) = &profile {
            create_foveated(session, &info, profile)?
        } else {
            session.create_swapchain(&info)?
        };
        handles.push(handle);
    }
    Ok(EyeSwapchains {
        handles,
        profile,
        effective: level,
    })
}

fn create_foveated(
    session: &xr::Session<xr::OpenGlEs>,
    info: &xr::SwapchainCreateInfo<xr::OpenGlEs>,
    profile: &xr::FoveationProfileFB,
) -> xr::Result<xr::Swapchain<xr::OpenGlEs>> {
    // openxr 0.21.1 create_swapchain hardcodes next=null. Mirror its lowering,
    // adding only the GLES scaled-bin extension; transfer ownership to the safe
    // wrapper immediately so subsequent errors destroy the acquired handle.
    let extra = xr::sys::SwapchainCreateInfoFoveationFB {
        ty: xr::sys::SwapchainCreateInfoFoveationFB::TYPE,
        next: std::ptr::null_mut(),
        flags: xr::SwapchainCreateFoveationFlagsFB::SCALED_BIN,
    };
    let raw = xr::sys::SwapchainCreateInfo {
        ty: xr::sys::SwapchainCreateInfo::TYPE,
        next: (&extra as *const xr::sys::SwapchainCreateInfoFoveationFB).cast(),
        create_flags: info.create_flags,
        usage_flags: info.usage_flags,
        format: info.format.into(),
        sample_count: info.sample_count,
        width: info.width,
        height: info.height,
        face_count: info.face_count,
        array_size: info.array_size,
        mip_count: info.mip_count,
    };
    let mut handle = xr::sys::Swapchain::NULL;
    // SAFETY: next-chain lives through the synchronous call; session is live.
    check("create_swapchain", unsafe {
        (session.instance().fp().create_swapchain)(session.as_raw(), &raw, &mut handle)
    })?;
    let swapchain = unsafe { xr::Swapchain::from_raw(session.clone(), handle) };
    let update = session
        .instance()
        .exts()
        .fb_swapchain_update_state
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let state = xr::sys::SwapchainStateFoveationFB {
        ty: xr::sys::SwapchainStateFoveationFB::TYPE,
        next: std::ptr::null_mut(),
        flags: xr::SwapchainStateFoveationFlagsFB::EMPTY,
        profile: profile.as_raw(),
    };
    // SAFETY: correctly typed state header, live swapchain and profile.
    check("update_swapchain", unsafe {
        (update.update_swapchain)(
            handle,
            (&state as *const xr::sys::SwapchainStateFoveationFB).cast(),
        )
    })?;
    // Updating copies the configuration: OpenXR even permits destroying the
    // profile immediately afterwards. Do not require a queried state's handle
    // to equal the source profile. Benchmarks independently verify the applied
    // level in the application's VrApi telemetry.
    Ok(swapchain)
}

fn check(operation: &str, result: xr::sys::Result) -> xr::Result<()> {
    if result.into_raw() < 0 {
        println!("SHOCK2QUEST_FFR_CALL_FAILED operation={operation} error={result:?}");
        Err(result)
    } else {
        Ok(())
    }
}
