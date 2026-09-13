//! Optional reconstruction underlay for the glove fit scene.
use openxr as xr;

pub struct FitPassthrough {
    supported: bool,
    attempted: bool,
    running: Option<RunningPassthrough>,
}

struct RunningPassthrough {
    // Drop the layer before its parent feature (Rust drops fields in order).
    layer: xr::PassthroughLayerFB,
    _feature: xr::Passthrough,
}

impl FitPassthrough {
    pub fn new(instance: &xr::Instance, system: xr::SystemId) -> Self {
        let supported = instance.exts().fb_passthrough.is_some()
            && supports_passthrough(instance, system).unwrap_or_else(|error| {
                println!("SHOCK2QUEST_PASSTHROUGH_SUPPORT_FAILED error={error:?}");
                false
            });
        Self {
            supported,
            attempted: false,
            running: None,
        }
    }

    pub fn update(&mut self, session: &xr::Session<xr::OpenGlEs>, requested: bool) {
        if !requested {
            if self.running.take().is_some() {
                println!("SHOCK2QUEST_PASSTHROUGH state=stopped");
            }
            self.attempted = false;
            return;
        }
        if self.attempted {
            return;
        }
        self.attempted = true;
        if !self.supported {
            println!("SHOCK2QUEST_PASSTHROUGH state=unsupported fallback=black");
            return;
        }
        let result = (|| {
            // openxr 0.21.1's Passthrough::start mistakenly calls pause.
            // Start at creation instead, and destroy on exit rather than
            // relying on that wrapper for a later resume.
            let flags = xr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION;
            let feature = session.create_passthrough(flags)?;
            let layer = session.create_passthrough_layer(
                &feature,
                flags,
                xr::PassthroughLayerPurposeFB::RECONSTRUCTION,
            )?;
            Ok::<_, xr::sys::Result>(RunningPassthrough {
                layer,
                _feature: feature,
            })
        })();
        match result {
            Ok(running) => {
                self.running = Some(running);
                println!("SHOCK2QUEST_PASSTHROUGH state=running scene=debug_gloves");
            }
            Err(error) => {
                println!("SHOCK2QUEST_PASSTHROUGH state=failed fallback=black error={error:?}")
            }
        }
    }

    pub fn layer(&self) -> Option<&xr::PassthroughLayerFB> {
        self.running.as_ref().map(|running| &running.layer)
    }
}

fn supports_passthrough(instance: &xr::Instance, system: xr::SystemId) -> xr::Result<bool> {
    let mut capabilities = xr::sys::SystemPassthroughProperties2FB {
        ty: xr::sys::SystemPassthroughProperties2FB::TYPE,
        next: std::ptr::null(),
        capabilities: xr::PassthroughCapabilityFlagsFB::EMPTY,
    };
    // SAFETY: both output structures live through the synchronous call. The
    // extension is enabled before this chain is passed to OpenXR.
    unsafe {
        let mut properties = xr::sys::SystemProperties::out(&mut capabilities as *mut _ as _);
        let result = (instance.fp().get_system_properties)(
            instance.as_raw(),
            system,
            properties.as_mut_ptr(),
        );
        if result.into_raw() < 0 {
            return Err(result);
        }
    }
    Ok(capabilities
        .capabilities
        .contains(xr::PassthroughCapabilityFlagsFB::PASSTHROUGH_CAPABILITY))
}

pub fn is_fit_scene(app: &shock2vr::App) -> bool {
    matches!(app, shock2vr::App::Ready(game) if game.scene_name() == "debug_gloves")
}
