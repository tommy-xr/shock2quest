//! Tracked hand joints for the in-VR grip pose tool, optionally alongside a
//! held controller (Meta multimodal). Controllers still drive play.
use std::ptr;

use openxr as xr;
use xr::sys::Handle;

/// Hand tracking plus, when the runtime offers them, optical-only joints and
/// simultaneous hands and controllers.
pub fn enable_extensions(available: &xr::ExtensionSet, enabled: &mut xr::ExtensionSet) {
    enabled.ext_hand_tracking = available.ext_hand_tracking;
    enabled.ext_hand_tracking_data_source =
        available.ext_hand_tracking && available.ext_hand_tracking_data_source;
    enabled.meta_simultaneous_hands_and_controllers =
        available.ext_hand_tracking && available.meta_simultaneous_hands_and_controllers;
    enabled.meta_detached_controllers =
        enabled.meta_simultaneous_hands_and_controllers && available.meta_detached_controllers;
}

/// One tracker per hand, or none without runtime support. With the data
/// source extension only optical tracking is requested, so a hand holding a
/// controller reports nothing rather than joints synthesized from it.
pub fn create_trackers<G>(
    instance: &xr::Instance,
    system: xr::SystemId,
    session: &xr::Session<G>,
) -> [Option<xr::HandTracker>; 2] {
    if !instance.supports_hand_tracking(system).unwrap_or(false) {
        return [None, None];
    }
    [xr::Hand::LEFT, xr::Hand::RIGHT].map(|hand| {
        if instance.exts().ext_hand_tracking_data_source.is_none() {
            return session.create_hand_tracker(hand).ok();
        }
        let fp = instance.exts().ext_hand_tracking.as_ref()?;
        let mut sources = [xr::sys::HandTrackingDataSourceEXT::UNOBSTRUCTED];
        let source_info = xr::sys::HandTrackingDataSourceInfoEXT {
            ty: xr::sys::HandTrackingDataSourceInfoEXT::TYPE,
            next: ptr::null(),
            requested_data_source_count: sources.len() as u32,
            requested_data_sources: sources.as_mut_ptr(),
        };
        let info = xr::sys::HandTrackerCreateInfoEXT {
            ty: xr::sys::HandTrackerCreateInfoEXT::TYPE,
            next: &source_info as *const _ as *const _,
            hand,
            hand_joint_set: xr::sys::HandJointSetEXT::DEFAULT,
        };
        let mut handle = xr::sys::HandTrackerEXT::NULL;
        // SAFETY: `info` and its chained source list outlive the call; the
        // handle belongs to `session`, which `from_raw` ties it to.
        unsafe {
            ((fp.create_hand_tracker)(session.as_raw(), &info, &mut handle)
                == xr::sys::Result::SUCCESS)
                .then(|| xr::HandTracker::from_raw(session, handle))
        }
    })
}

/// Turn on simultaneous hands and controllers when the system supports it:
/// one controller stays in hand while the other hand is tracked. The mode
/// resets with every session, so call this per session.
pub fn enable_multimodal<G>(
    instance: &xr::Instance,
    system: xr::SystemId,
    session: &xr::Session<G>,
) -> bool {
    let Some(fp) = instance
        .exts()
        .meta_simultaneous_hands_and_controllers
        .as_ref()
    else {
        return false;
    };
    // SAFETY: the property struct is chained for this one call and read
    // only after it succeeds.
    let supported = unsafe {
        let mut props =
            xr::sys::SystemSimultaneousHandsAndControllersPropertiesMETA::out(ptr::null_mut());
        let mut system_props = xr::sys::SystemProperties::out(&mut props as *mut _ as _);
        (instance.fp().get_system_properties)(instance.as_raw(), system, system_props.as_mut_ptr())
            == xr::sys::Result::SUCCESS
            && props
                .assume_init()
                .supports_simultaneous_hands_and_controllers
                .into()
    };
    if !supported {
        return false;
    }
    let resume = xr::sys::SimultaneousHandsAndControllersTrackingResumeInfoMETA {
        ty: xr::sys::SimultaneousHandsAndControllersTrackingResumeInfoMETA::TYPE,
        next: ptr::null(),
    };
    // SAFETY: a valid session and resume info for the duration of the call.
    unsafe {
        (fp.resume_simultaneous_hands_and_controllers_tracking)(session.as_raw(), &resume)
            == xr::sys::Result::SUCCESS
    }
}
