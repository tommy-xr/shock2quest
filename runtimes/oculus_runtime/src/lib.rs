extern crate gl;
extern crate khronos_egl as egl;

use cgmath::Quaternion;
use cgmath::vec2;
use engine::profile;
use engine::scene::Scene;
use engine::scene::SceneObject;
use openxr as xr;
use shock2vr::App;
use shock2vr::GameOptions;
use shock2vr::input_context::InputContext;
use shock2vr::paths;
use shock2vr::vr_crouch::VrCrouchDetector;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use std::cell::RefCell;
use std::env;

use tracing;

mod android_permissions;
mod debug_input;
mod frame_profiler;
mod quest_config;
mod refresh_rate;

use tokio::runtime::Runtime;

// Fix to get c++ shared (for __cxa_pure_virtual issue):
// - https://github.com/RustAudio/cpal/issues/563
// - https://github.com/rust-mobile/cargo-apk/issues/13
#[cfg_attr(target_os = "android", link(name = "c++_shared"))]
unsafe extern "C" {}

#[cfg_attr(target_os = "android", ndk_glue::main)]
fn main() {
    #[cfg(feature = "linked")]
    let entry = xr::Entry::linked();
    #[cfg(not(feature = "linked"))]
    // SAFETY: the APK packages Meta's OpenXR-conformant loader as
    // `libopenxr_loader.so`.
    let entry = unsafe { xr::Entry::load() }
        .expect("couldn't find the OpenXR loader; try enabling the \"static\" feature");

    #[cfg(target_os = "android")]
    let _ = entry.initialize_android_loader();
    let rt = Runtime::new().unwrap();
    let permission_granted = rt.block_on(async move {
        println!("hello from the async block");
        let result = android_permissions::request_permission().await;
        match result {
            Ok(granted) => {
                if granted {
                    println!("Permissions granted!");
                    true
                } else {
                    println!("Permissions denied!");
                    false
                }
            }
            Err(e) => {
                println!("Error requesting permissions: {:?}", e);
                false
            }
        }
    });

    if !permission_granted {
        println!("Cannot access storage without permissions");
        return;
    }

    println!(
        "after async: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );

    let test_dir = paths::data_root().join("res/obj/txt16");
    println!("Trying to read directory: {}", test_dir.display());

    if let Ok(paths) = std::fs::read_dir(&test_dir) {
        for path in paths {
            println!("Name: {}", path.unwrap().path().display())
        }
    } else {
        println!("Failed to read directory: {}", test_dir.display());
    }

    // println!("Trying to read file...");
    // let test = std::fs::File::open("/mnt/sdcard/shock2quest/res/obj/txt16/LOG.PCX").unwrap();
    // println!("Read file successfully!");

    let extensions = entry.enumerate_extensions().unwrap();
    println!("supported extensions: {:#?}", extensions);
    let layers = entry.enumerate_layers().unwrap();
    println!("supported layers: {:?}", layers);
    // OpenXR will fail to initialize if we ask for an extension that OpenXR can't provide! So we
    // need to check all our extensions before initializing OpenXR with them. Note that even if the
    // extension is present, it's still possible you may not be able to use it. For example: the
    // hand tracking extension may be present, but the hand sensor might not be plugged in or turned
    // on. There are often additional checks that should be made before using certain features!
    let available_extensions = entry.enumerate_extensions().unwrap();

    // If a required extension isn't present, you want to ditch out here! It's possible something
    // like your rendering API might not be provided by the active runtime. APIs like OpenGL don't
    // have universal support.
    assert!(available_extensions.khr_opengl_es_enable);
    assert!(available_extensions.fb_display_refresh_rate);

    // Initialize OpenXR with the extensions we've found!
    let mut enabled_extensions = xr::ExtensionSet::default();
    enabled_extensions.khr_opengl_es_enable = true;
    enabled_extensions.fb_display_refresh_rate = true;
    #[cfg(target_os = "android")]
    {
        enabled_extensions.khr_android_create_instance = true;
    }
    let xr_instance = entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: "openxrs example",
                application_version: 0,
                engine_name: "openxrs example",
                engine_version: 0,
                api_version: xr::Version::new(1, 0, 0),
            },
            &enabled_extensions,
            &[],
        )
        .unwrap();
    let instance_props = xr_instance.properties().unwrap();
    println!(
        "loaded OpenXR runtime: {} {}",
        instance_props.runtime_name, instance_props.runtime_version
    );
    let system = xr_instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .unwrap();
    // Check what blend mode is valid for this device (opaque vs transparent displays). We'll just
    // take the first one available!
    let environment_blend_mode = xr_instance
        .enumerate_environment_blend_modes(system, VIEW_TYPE)
        .unwrap()[0];
    let system_props = xr_instance.system_properties(system).unwrap();
    println!(
        "selected system {}: {}",
        system_props.system_id.into_raw(),
        if system_props.system_name.is_empty() {
            "<unnamed>"
        } else {
            &system_props.system_name
        }
    );

    let view_config_views = xr_instance
        .enumerate_view_configuration_views(system, xr::ViewConfigurationType::PRIMARY_STEREO)
        .unwrap();
    println!("view configuration views: {:#?}", view_config_views);

    let reqs = xr_instance
        .graphics_requirements::<xr::OpenGlEs>(system)
        .unwrap();

    println!(
        "min_supported: {} max supported: {}",
        reqs.min_api_version_supported.into_raw(),
        reqs.max_api_version_supported.into_raw()
    );

    let lib = unsafe { libloading::Library::new("libEGL.so").expect("unable to find libEGL.so") };
    let egl = unsafe {
        egl::DynamicInstance::<egl::EGL1_4>::load_required_from(lib)
            .expect("unable to load libEGL.so.1")
    };
    let _attributes = [
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::NONE,
    ];

    let egl_display = egl.get_display(0 as egl::NativeDisplayType).unwrap();
    // if egl_display.is_some() {
    //     println!("Got a display!");
    // } else {
    //     println!("NO DISPLAY!");
    // }
    println!("Got display");

    egl.initialize(egl_display).unwrap();
    println!("Initialized!");

    let mut configs = Vec::with_capacity(1024);

    let _ = egl.get_configs(egl_display, &mut configs);
    println!("configs: {:?}", &configs);
    let attributes = [
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::DEPTH_SIZE,
        0,
        egl::STENCIL_SIZE,
        0,
        egl::SAMPLES,
        0,
        egl::NONE,
    ];
    // TODO: Manully select config!
    // Because:
    // Do NOT use eglChooseConfig, because the Android EGL code pushes in multisample
    // flags in eglChooseConfig if the user has selected the "force 4x MSAA" option in
    // settings, and that is completely wasted for our warp target.
    let config = egl
        .choose_first_config(egl_display, &attributes)
        .unwrap()
        .unwrap();
    println!("Got config!");
    let context_attributes = [
        egl::CONTEXT_MAJOR_VERSION,
        3,
        egl::CONTEXT_MINOR_VERSION,
        2,
        egl::NONE,
    ];
    let context = egl
        .create_context(egl_display, config, None, &context_attributes)
        .unwrap();
    println!("Created context");

    // Create a test pbuffer
    let surface_attributes = [egl::WIDTH, 16, egl::HEIGHT, 16, egl::NONE];
    let tiny_surface = egl
        .create_pbuffer_surface(egl_display, config, &surface_attributes)
        .unwrap();
    println!("Created surface!");

    egl.make_current(
        egl_display,
        Some(tiny_surface),
        Some(tiny_surface),
        Some(context),
    )
    .unwrap();

    unsafe {
        let mut major_version = 0;
        let mut minor_version = 0;
        gl::load_with(|s| match egl.get_proc_address(s) {
            None => 0 as *const _,
            Some(v) => v as *const _,
        });
        gl::GetIntegerv(gl::MAJOR_VERSION, &mut major_version);
        gl::GetIntegerv(gl::MINOR_VERSION, &mut minor_version);
        println!("Major: {} Minor: {}", major_version, minor_version);
    }

    let system_id = xr_instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .unwrap();
    println!("System ID: {:?}", system_id);
    // A session represents this application's desire to display things! This is where we hook
    // up our graphics API. This does not start the session; for that, you'll need a call to
    // Session::begin, which we do in 'main_loop below.
    let session_create_info = &xr::opengles::SessionCreateInfo::Android {
        context: context.as_ptr(),
        display: egl_display.as_ptr(),
        config: config.as_ptr(),
    };
    let (session, mut frame_wait, mut frame_stream) = unsafe {
        xr_instance
            .create_session::<xr::OpenGlEs>(system, session_create_info)
            .unwrap()
    };

    // Create a stage!
    let stage = session
        .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
        .unwrap();

    let head_space = session
        .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
        .unwrap();

    let _local_space = session
        .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
        .unwrap();

    let _right_hand_path = xr_instance.string_to_path("/user/hand/right").unwrap();
    let action_set = xr_instance
        .create_action_set("main", "main action set", 0)
        .unwrap();

    let left_grip = action_set
        .create_action::<xr::Posef>("left_grip", "Left Hand Grip", &[])
        .unwrap();

    let right_grip = action_set
        .create_action::<xr::Posef>("right_grip", "Right Hand Grip", &[])
        .unwrap();

    let left_aim = action_set
        .create_action::<xr::Posef>("left_aim", "Left Hand Aim", &[])
        .unwrap();

    let right_aim = action_set
        .create_action::<xr::Posef>("right_aim", "Right Hand Aim", &[])
        .unwrap();

    let left_trigger = action_set
        .create_action::<f32>("left_trigger", "Left Hand Trigger", &[])
        .unwrap();

    let right_trigger = action_set
        .create_action::<f32>("right_trigger", "Right Hand Trigger", &[])
        .unwrap();

    let left_squeeze = action_set
        .create_action::<f32>("left_squeeze", "Left Hand Squeeze", &[])
        .unwrap();

    let right_squeeze = action_set
        .create_action::<f32>("right_squeeze", "Right Hand Squeeze", &[])
        .unwrap();

    let left_thumbstick_action = action_set
        .create_action::<xr::Vector2f>("left_hand_thumbstick", "Left Hand Thumbstick", &[])
        .unwrap();

    let right_thumbstick_action = action_set
        .create_action::<xr::Vector2f>("right_hand_thumbstick", "Right Hand Thumbstick", &[])
        .unwrap();

    let jump_action = action_set
        .create_action::<bool>("jump", "Jump", &[])
        .unwrap();

    let crouch_action = action_set
        .create_action::<bool>("crouch", "Crouch Toggle", &[])
        .unwrap();

    let inventory_action = action_set
        .create_action::<bool>("inventory", "Backpack Inventory", &[])
        .unwrap();

    let audio_log_action = action_set
        .create_action::<bool>("audio_log_reader", "Audio Log Reader", &[])
        .unwrap();

    // The left controller's Menu button. (The right controller's is reserved
    // by the Quest system UI, so it can never be the app's.)
    let menu_action = action_set
        .create_action::<bool>("menu", "Pause Menu", &[])
        .unwrap();

    // Bind our actions to input devices using the given profile
    // If you want to access inputs specific to a particular device you may specify a different
    // interaction profile
    xr_instance
        .suggest_interaction_profile_bindings(
            xr_instance
                .string_to_path("/interaction_profiles/oculus/touch_controller")
                .unwrap(),
            &[
                xr::Binding::new(
                    &left_grip,
                    xr_instance
                        .string_to_path("/user/hand/left/input/grip/pose")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &right_grip,
                    xr_instance
                        .string_to_path("/user/hand/right/input/grip/pose")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &left_aim,
                    xr_instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &right_aim,
                    xr_instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &left_trigger,
                    xr_instance
                        .string_to_path("/user/hand/left/input/trigger/value")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &right_trigger,
                    xr_instance
                        .string_to_path("/user/hand/right/input/trigger/value")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &left_squeeze,
                    xr_instance
                        .string_to_path("/user/hand/left/input/squeeze/value")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &right_squeeze,
                    xr_instance
                        .string_to_path("/user/hand/right/input/squeeze/value")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &left_thumbstick_action,
                    xr_instance
                        .string_to_path("/user/hand/left/input/thumbstick")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &right_thumbstick_action,
                    xr_instance
                        .string_to_path("/user/hand/right/input/thumbstick")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &jump_action,
                    xr_instance
                        .string_to_path("/user/hand/right/input/thumbstick/click")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &crouch_action,
                    xr_instance
                        .string_to_path("/user/hand/left/input/thumbstick/click")
                        .unwrap(),
                ),
                xr::Binding::new(
                    &inventory_action,
                    xr_instance
                        .string_to_path(
                            shock2vr::input::InputAction::MoveInventory
                                .quest_touch_click_path()
                                .expect("Quest backpack binding"),
                        )
                        .unwrap(),
                ),
                xr::Binding::new(
                    &audio_log_action,
                    xr_instance
                        .string_to_path(
                            shock2vr::input::InputAction::ReadLastUnreadLog
                                .quest_touch_click_path()
                                .expect("Quest audio-log binding"),
                        )
                        .unwrap(),
                ),
                xr::Binding::new(
                    &menu_action,
                    xr_instance
                        .string_to_path(
                            shock2vr::input::InputAction::TogglePauseMenu
                                .quest_touch_click_path()
                                .expect("Quest pause-menu binding"),
                        )
                        .unwrap(),
                ),
            ],
        )
        .unwrap();

    // Attach the action set to the session
    session.attach_action_sets(&[&action_set]).unwrap();

    // Create an action space for each device we want to locate
    let right_aim_space = right_aim
        .create_space(&session, xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();

    let left_aim_space = left_aim
        .create_space(&session, xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();

    // Main loop
    let mut swapchain = None;
    let mut event_storage = xr::EventDataBuffer::new();
    let mut session_running = false;
    // Set once a scene has asked to quit and we've asked OpenXR to exit, so the
    // request is made exactly once (the runtime takes several frames to answer).
    let mut exit_requested = false;
    let now = Instant::now();
    let engine = engine::android();
    let bundle_storage = engine.get_storage();
    let mut experimental_features = HashSet::new();
    // experimental_features.insert("gui".to_owned());
    // Level transitions are deferred so the loading screen shows while the level
    // parses, instead of freezing the headset on the last menu frame for the whole
    // load (#1002). On by default here because a frozen compositor frame is far
    // worse in a headset than on a monitor; it stays opt-in on the desktop.
    experimental_features.insert("loading_screen".to_owned());
    let mission = quest_config::configured_mission();
    let game_init_started = Instant::now();
    let options: GameOptions = GameOptions {
        render_particles: false,
        mission: mission.clone(),
        experimental_features,
        debug_skeletons: false,
        ..GameOptions::default()
    };
    // NativeActivity owns Android lifecycle and input queues on this thread.
    // Keep servicing them at milestones inside the synchronous first load,
    // before the per-frame pump below exists.
    #[cfg(target_os = "android")]
    {
        // `set_event_pump` takes a plain `fn()`; the drain's "was the activity
        // destroyed" answer is only read at teardown.
        fn pump_events() {
            android_pump_events();
        }
        engine::platform::set_event_pump(Some(pump_events));
    }
    let mut game = shock2vr::App::init(options, bundle_storage);
    // The real HMD orientation, from the previous frame's located view.
    // `input_context` is built before `locate_views` runs, so this frame's view
    // pose does not exist yet; one frame of latency is imperceptible for
    // head-anchored UI and is far better than the alternative below.
    let mut last_view_rotation: Option<cgmath::Quaternion<f32>> = None;
    // Where that same view was, already converted to pawn space (the space the
    // hands and the world-anchored frontend panel live in).
    let mut last_view_position: Option<cgmath::Vector3<f32>> = None;
    println!(
        "SHOCK2QUEST_STARTUP mission={} init_ms={:.3}",
        mission,
        game_init_started.elapsed().as_secs_f64() * 1_000.0
    );

    // Controller button edges feed the same semantic action dispatcher as the
    // desktop and debug runtimes. X exposes the player-owned backpack panel;
    // Y opens/closes and replays the player-owned audio-log reader; the left
    // Menu button opens/closes the pause menu.
    // (The debug input server below can inject the same actions remotely.)
    let mut action_state = shock2vr::input::InputActionState::new();
    // Opt-in remote control for on-device automation; `None` unless
    // /sdcard/shock2quest/debug-port.txt configures a port.
    let debug_input = debug_input::DebugInputServer::start_if_configured(&mission);
    let mut vr_crouch = VrCrouchDetector::default();
    let mut pending_stage_change_time = None;
    // Button-crouch alternative to the physical detector: left thumbstick
    // click toggles a latched crouch request (mirroring jump on the right
    // stick). Either source requests the crouch; the game's headroom-gated
    // stand-up still decides when standing is actually possible.
    let mut crouch_toggled = false;
    let mut crouch_button_was_pressed = false;

    let _camera_pos = vec3(0.0, 5.0, 10.0);

    let render_time = Instant::now();
    let mut last_update_time = render_time;
    let mut frame_profiler = frame_profiler::FrameProfiler::new(Duration::from_secs(1));
    let mut display_refresh_rate = None;
    let mut requested_display_refresh_rate = None;
    let mut ready_reported = false;
    let mut session_focused = false;
    // Consecutive rejected frame submissions, and consecutive frames whose
    // views were not tracked. Both are logged once at the start of a burst and
    // once on recovery (with the length), so a long outage is still visible in
    // logcat without printing at the display refresh rate.
    let mut submit_failures: u64 = 0;
    let mut untracked_view_frames: u64 = 0;
    'main_loop: loop {
        // Drain Android's NativeActivity queues. OpenXR is the real input
        // path - nothing here feeds gameplay - but NativeActivity hands the
        // app a lifecycle event pipe and an input queue, and leaving them
        // unconsumed is what makes Horizon OS raise "shock2quest isn't
        // responding" while the OpenXR session renders happily at 90 Hz.
        #[cfg(target_os = "android")]
        android_pump_events();

        // println!(
        //     " - Before polling events: {}",
        //     render_time.elapsed().as_secs_f32()
        // );
        while let Some(event) = xr_instance.poll_event(&mut event_storage).unwrap() {
            use xr::Event::*;
            match event {
                SessionStateChanged(e) => {
                    // Session state change is where we can begin and end sessions, as well as
                    // find quit messages!
                    println!("entered state {:?}", e.state());
                    println!(
                        "SHOCK2QUEST_XR_STATE mission={} state={:?}",
                        mission,
                        e.state()
                    );
                    let next_session_focused = e.state() == xr::SessionState::FOCUSED;
                    if next_session_focused != session_focused {
                        // Never emit a one-second bucket containing samples from
                        // both sides of a focus transition.
                        frame_profiler.reset();
                    }
                    session_focused = next_session_focused;
                    match e.state() {
                        xr::SessionState::READY => {
                            session.begin(VIEW_TYPE).unwrap();
                            session_running = true;
                            let available_refresh_rates =
                                session.enumerate_display_refresh_rates().unwrap();
                            let advertised_refresh_rate = refresh_rate::select_supported_rate(
                                &available_refresh_rates,
                                refresh_rate::TARGET_HZ,
                            );
                            println!(
                                "SHOCK2QUEST_REFRESH_RATES target_hz={:.3} available_hz={available_refresh_rates:?}",
                                refresh_rate::TARGET_HZ
                            );
                            // Horizon OS can return an empty advertised-rate list
                            // immediately after session begin. In that case, let
                            // xrRequestDisplayRefreshRateFB authoritatively accept or
                            // reject the target instead of silently inheriting a default.
                            let request_candidate = advertised_refresh_rate.or_else(|| {
                                available_refresh_rates
                                    .is_empty()
                                    .then_some(refresh_rate::TARGET_HZ)
                            });
                            requested_display_refresh_rate =
                                request_candidate.and_then(|requested_hz| {
                                    match session.request_display_refresh_rate(requested_hz) {
                                        Ok(()) => Some(requested_hz),
                                        Err(
                                            error
                                            @ xr::sys::Result::ERROR_DISPLAY_REFRESH_RATE_UNSUPPORTED_FB,
                                        ) if available_refresh_rates.is_empty() => {
                                            println!(
                                                "SHOCK2QUEST_REFRESH_REQUEST requested_hz={requested_hz:.3} result=unsupported error={error:?}"
                                            );
                                            None
                                        }
                                        Err(error) => {
                                            panic!(
                                                "failed to request advertised refresh rate {requested_hz:.3}: {error:?}"
                                            );
                                        }
                                    }
                                });
                            display_refresh_rate =
                                Some(session.get_display_refresh_rate().unwrap_or_else(|error| {
                                    panic!("failed to query active display refresh rate: {error:?}")
                                }));
                            ready_reported = false;
                            last_update_time = Instant::now();
                            frame_profiler.reset();
                            vr_crouch.reset();
                            pending_stage_change_time = None;
                            // A latched button crouch must not survive a
                            // session restart (doffing the headset would
                            // otherwise resume invisibly crouched).
                            crouch_toggled = false;
                            crouch_button_was_pressed = false;
                            action_state.release(shock2vr::input::InputAction::MoveInventory);
                            action_state.release(shock2vr::input::InputAction::ReadLastUnreadLog);
                        }
                        xr::SessionState::STOPPING => {
                            session.end().unwrap();
                            session_running = false;
                            last_update_time = Instant::now();
                            frame_profiler.reset();
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            break 'main_loop;
                        }
                        _ => {}
                    }
                }
                InstanceLossPending(_) => {
                    break 'main_loop;
                }
                ReferenceSpaceChangePending(e)
                    if e.reference_space_type() == xr::ReferenceSpaceType::STAGE =>
                {
                    // The new floor origin applies at change_time, not when
                    // this event is delivered. Recalibrate on the first frame
                    // whose tracked poses use that new STAGE definition.
                    pending_stage_change_time = Some(e.change_time());
                }
                EventsLost(e) => {
                    println!("lost {} events", e.lost_event_count());
                }
                DisplayRefreshRateChangedFB(e) => {
                    display_refresh_rate = Some(e.to_display_refresh_rate());
                    println!(
                        "SHOCK2QUEST_REFRESH_CHANGED from_hz={:.3} to_hz={:.3}",
                        e.from_display_refresh_rate(),
                        e.to_display_refresh_rate()
                    );
                }
                _ => {}
            }
        }
        if !session_running {
            // Don't grind up the CPU
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        // A scene asked to quit (the main menu's Quit item). OpenXR owns
        // teardown: xrRequestExitSession makes the runtime walk us through
        // STOPPING - where the handler above ends the session - to EXITING,
        // which breaks 'main_loop. `should_quit` latches, so it is read here,
        // outside the wait/begin/end frame sequence: the failure path below
        // leaves the loop, and doing that mid-frame would strand an
        // xrWaitFrame with no matching xrBeginFrame.
        if !exit_requested && game.should_quit() {
            exit_requested = true;
            println!("SHOCK2QUEST_XR_EXIT_REQUESTED");
            if let Err(error) = session.request_exit() {
                // No orderly path left; leave the loop and finish the activity
                // rather than lingering on a session nobody can end.
                println!("SHOCK2QUEST_XR_EXIT_REQUEST_FAILED error={error:?}");
                break 'main_loop;
            }
        }
        // println!(
        //     " - After polling events: {}",
        //     render_time.elapsed().as_secs_f32()
        // );

        // Block until the previous frame is finished displaying, and is ready for another one.
        // Also returns a prediction of when the next frame will be displayed, for use with
        // predicting locations of controllers, viewpoints, etc.
        let xr_frame_state = frame_wait.wait().unwrap();

        if let Some(change_time) = pending_stage_change_time {
            if xr_frame_state.predicted_display_time.as_nanos() >= change_time.as_nanos() {
                vr_crouch.reset();
                pending_stage_change_time = None;
                // The reference space was redefined; drop the latched button
                // crouch along with the height calibration.
                crouch_toggled = false;
                crouch_button_was_pressed = false;
            }
        }

        let current = Instant::now();
        let total_time = current - render_time;
        let elapsed_time = current - last_update_time;
        last_update_time = current;
        let time_context = shock2vr::time::Time {
            elapsed: elapsed_time,
            total: total_time,
        };

        session.sync_actions(&[(&action_set).into()]).unwrap();
        // Find where our controllers are located in the Stage space
        let left_aim_location = left_aim_space
            .locate(&stage, xr_frame_state.predicted_display_time)
            .unwrap();
        let right_aim_location = right_aim_space
            .locate(&stage, xr_frame_state.predicted_display_time)
            .unwrap();
        let head_location = head_space
            .locate(&stage, xr_frame_state.predicted_display_time)
            .unwrap();

        let left_thumbstick_value = left_thumbstick_action
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let right_thumbstick_value = right_thumbstick_action
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let jump_pressed = jump_action
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let crouch_state = crouch_action.state(&session, xr::Path::NULL).unwrap();
        let inventory_state = inventory_action.state(&session, xr::Path::NULL).unwrap();
        let audio_log_state = audio_log_action.state(&session, xr::Path::NULL).unwrap();
        let menu_state = menu_action.state(&session, xr::Path::NULL).unwrap();
        // Only edge-detect while the action is live: with the session merely
        // VISIBLE (system overlay up), current_state reads false even though
        // the button may still be physically held, and treating that as a
        // release would mint a spurious toggle on refocus.
        if crouch_state.is_active {
            if crouch_state.current_state && !crouch_button_was_pressed {
                crouch_toggled = !crouch_toggled;
            }
            crouch_button_was_pressed = crouch_state.current_state;
        }
        action_state.sync_discrete_button(
            shock2vr::input::InputAction::MoveInventory,
            inventory_state.is_active,
            inventory_state.changed_since_last_sync,
            inventory_state.current_state,
        );
        action_state.sync_discrete_button(
            shock2vr::input::InputAction::ReadLastUnreadLog,
            audio_log_state.is_active,
            audio_log_state.changed_since_last_sync,
            audio_log_state.current_state,
        );
        action_state.sync_discrete_button(
            shock2vr::input::InputAction::TogglePauseMenu,
            menu_state.is_active,
            menu_state.changed_since_last_sync,
            menu_state.current_state,
        );

        let left_trigger_value = left_trigger
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let right_trigger_value = right_trigger
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;

        let left_squeeze_value = left_squeeze
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let right_squeeze_value = right_squeeze
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;

        let _speed = 50.0;

        // let forward_xr = right_aim_location.pose.orientation;
        // //let forward_xr = views[0].pose.orientation;
        // let dir = cgmath::Quaternion::new(forward_xr.w, forward_xr.x, forward_xr.y, forward_xr.z);

        // let forward = dir.rotate_vector(vec3(0.0, 0.0, -time * right_trigger_value * 10.));
        // The RIGHT CONTROLLER's aim pose. It drives hand aiming and, for want
        // of anything else, has also been standing in for the head - which is
        // wrong for anything that wants to know where the player is *looking*,
        // and is the zero quaternion entirely when the controllers are not
        // tracked.
        let aim_rotation = cgmath::Quaternion::new(
            right_aim_location.pose.orientation.w,
            right_aim_location.pose.orientation.x,
            right_aim_location.pose.orientation.y,
            right_aim_location.pose.orientation.z,
        );
        // ...so the head gets the actual head. Before any view has been
        // located (frame 0) the HEAD SPACE pose - located above, this frame -
        // stands in; the controller aim is only the last resort, when neither
        // is available. That matters now that the frontend panel is *placed
        // once* off this pose: anchoring the boot menu to wherever a controller
        // happened to point would strand it there for the whole screen.
        let head_pose_rotation = head_location
            .location_flags
            .contains(
                xr::SpaceLocationFlags::ORIENTATION_VALID
                    | xr::SpaceLocationFlags::ORIENTATION_TRACKED,
            )
            .then(|| {
                cgmath::Quaternion::new(
                    head_location.pose.orientation.w,
                    head_location.pose.orientation.x,
                    head_location.pose.orientation.y,
                    head_location.pose.orientation.z,
                )
            });
        let head_rotation = last_view_rotation
            .or(head_pose_rotation)
            .unwrap_or(aim_rotation);

        // Feed the detector before the poses are transformed so this frame's
        // physical stance is available to the crouch request below. Tracked
        // poses are NEVER artificially displaced for a button crouch: the eye
        // cap in `render_swapchain` alone keeps the view inside the crouched
        // collider, and it does so continuously (a rigid pose drop keyed on
        // detector state produced below-floor eyes/hands and frame-size view
        // pops at the hysteresis thresholds).
        let tracked_head_position = head_location.location_flags.contains(
            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::POSITION_TRACKED,
        );
        let physically_crouched =
            vr_crouch.update(tracked_head_position.then_some(head_location.pose.position.y));
        let center_above_floor = game.player_center_above_floor();
        let right_hand_position = stage_to_pawn(
            vec3(
                right_aim_location.pose.position.x,
                right_aim_location.pose.position.y,
                right_aim_location.pose.position.z,
            ),
            center_above_floor,
        );

        let left_hand_position = stage_to_pawn(
            vec3(
                left_aim_location.pose.position.x,
                left_aim_location.pose.position.y,
                left_aim_location.pose.position.z,
            ),
            center_above_floor,
        );
        let left_hand_rotation = cgmath::Quaternion::new(
            left_aim_location.pose.orientation.w,
            left_aim_location.pose.orientation.x,
            left_aim_location.pose.orientation.y,
            left_aim_location.pose.orientation.z,
        );

        let mut input_context = InputContext::default();
        input_context.head.rotation = head_rotation;
        // The tracked eye, in pawn space. The located head space covers frame
        // 0, before any view has been located; when the head is untracked
        // entirely this keeps `Head::default`'s fixed eye height, which is
        // where the camera renders from anyway.
        let head_pose_position = tracked_head_position.then(|| {
            stage_to_pawn(
                vec3(
                    head_location.pose.position.x,
                    head_location.pose.position.y,
                    head_location.pose.position.z,
                ),
                center_above_floor,
            )
        });
        if let Some(position) = last_view_position.or(head_pose_position) {
            input_context.head.position = position;
        }
        input_context.right_hand.rotation = aim_rotation;
        input_context.right_hand.position = right_hand_position;
        input_context.right_hand.trigger_value = right_trigger_value;
        input_context.right_hand.squeeze_value = right_squeeze_value;
        input_context.right_hand.thumbstick =
            vec2(-right_thumbstick_value.x, right_thumbstick_value.y);

        input_context.left_hand.rotation = left_hand_rotation;
        input_context.left_hand.position = left_hand_position;
        input_context.left_hand.trigger_value = left_trigger_value;
        input_context.left_hand.squeeze_value = left_squeeze_value;
        input_context.left_hand.thumbstick =
            vec2(-left_thumbstick_value.x, left_thumbstick_value.y);
        input_context.jump = jump_pressed;
        // The detector was already fed exactly once above (it keeps its
        // standing calibration warm even while the button latch is active).
        input_context.crouch = physically_crouched || crouch_toggled;
        // Remote overrides are layered ON TOP of the live controller state, so
        // only the channels an agent claimed are replaced.
        if let Some(debug_input) = &debug_input {
            debug_input.apply(&mut input_context, &mut action_state);
        }
        let update_started = Instant::now();
        game.update(&time_context, &input_context, &mut action_state);
        let update_elapsed = update_started.elapsed();

        // Must be called before any rendering is done!
        frame_stream.begin().unwrap();

        // println!(
        //     " - After blocking for previous frame: {}",
        //     render_time.elapsed().as_secs_f32()
        // );

        if !xr_frame_state.should_render {
            //println!("Skipping frame!");
            end_frame_with_no_layers(
                &mut frame_stream,
                xr_frame_state.predicted_display_time,
                environment_blend_mode,
            );
            if let Some(report) = frame_profiler.record_skipped(elapsed_time, update_elapsed) {
                print_frame_report(&mission, session_focused, report);
            }
            continue;
        }

        let swapchain = swapchain.get_or_insert_with(|| {
            // Now we need to find all the viewpoints we need to take care of! This is a
            // property of the view configuration type; in this example we use PRIMARY_STEREO,
            // so we should have 2 viewpoints.
            //
            // Because we are using multiview in this example, we require that all view
            // dimensions are identical.
            //println!("Creating views...");
            let views = xr_instance
                .enumerate_view_configuration_views(system, VIEW_TYPE)
                .unwrap();
            assert_eq!(views.len(), VIEW_COUNT as usize);
            assert_eq!(views[0], views[1]);
            //println!("Views: {:#?}", views);

            let width = views[0].recommended_image_rect_width;
            let height = views[0].recommended_image_rect_height;

            let swapchain_handles = views
                .into_iter()
                .map(|view| {
                    let swapchain = session
                        .create_swapchain(&xr::SwapchainCreateInfo {
                            create_flags: xr::SwapchainCreateFlags::EMPTY,
                            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                                | xr::SwapchainUsageFlags::SAMPLED,
                            format: gl::SRGB8_ALPHA8,
                            // The Vulkan graphics pipeline we create is not set up for multisampling,
                            // so we hardcode this to 1. If we used a proper multisampling setup, we
                            // could set this to `views[0].recommended_swapchain_sample_count`.
                            sample_count: 1,
                            width: view.recommended_image_rect_width,
                            height: view.recommended_image_rect_height,
                            face_count: 1,
                            array_size: 1,
                            mip_count: 1,
                        })
                        .unwrap();

                    let images = swapchain.enumerate_images().unwrap();

                    let buffers = images
                        .into_iter()
                        .map(|image| {
                            unsafe {
                                gl::BindTexture(gl::TEXTURE_2D, image);
                                gl::TexParameteri(
                                    gl::TEXTURE_2D,
                                    gl::TEXTURE_WRAP_S,
                                    gl::CLAMP_TO_EDGE.try_into().unwrap(),
                                );
                                gl::TexParameteri(
                                    gl::TEXTURE_2D,
                                    gl::TEXTURE_WRAP_T,
                                    gl::CLAMP_TO_EDGE.try_into().unwrap(),
                                );
                                gl::TexParameteri(
                                    gl::TEXTURE_2D,
                                    gl::TEXTURE_MIN_FILTER,
                                    gl::LINEAR.try_into().unwrap(),
                                );
                                gl::TexParameteri(
                                    gl::TEXTURE_2D,
                                    gl::TEXTURE_MAG_FILTER,
                                    gl::LINEAR.try_into().unwrap(),
                                );
                                gl::BindTexture(gl::TEXTURE_2D, 0);

                                // Create a depth buffer
                                let mut depth_buffer: gl::types::GLuint = 0;
                                gl::GenRenderbuffers(1, &mut depth_buffer);
                                gl::BindRenderbuffer(gl::RENDERBUFFER, depth_buffer);
                                gl::RenderbufferStorage(
                                    gl::RENDERBUFFER,
                                    gl::DEPTH_COMPONENT24,
                                    width as i32,
                                    height as i32,
                                );

                                gl::BindRenderbuffer(gl::RENDERBUFFER, 0);

                                // Create the frame buffer.
                                let mut buffer: gl::types::GLuint = 0;
                                gl::GenFramebuffers(1, &mut buffer);
                                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, buffer);
                                gl::FramebufferTexture2D(
                                    gl::DRAW_FRAMEBUFFER,
                                    gl::COLOR_ATTACHMENT0,
                                    gl::TEXTURE_2D,
                                    image,
                                    0,
                                );
                                // Attach the depth buffer to the frame buffer
                                gl::FramebufferRenderbuffer(
                                    gl::DRAW_FRAMEBUFFER,
                                    gl::DEPTH_ATTACHMENT,
                                    gl::RENDERBUFFER,
                                    depth_buffer,
                                );
                                let _result = gl::CheckFramebufferStatus(gl::DRAW_FRAMEBUFFER);
                                // This app was originally written with the presumption that
                                // its swapchains and compositor front buffer were RGB.
                                // In order to have the colors the same now that its compositing
                                // to an sRGB front buffer, we have to write to an sRGB swapchain
                                // but with the linear->sRGB conversion disabled on write.
                                gl::Disable(gl::FRAMEBUFFER_SRGB);
                                Framebuffer {
                                    image,
                                    depth_buffer,
                                    gl_color_buffer: buffer,
                                }
                            }
                        })
                        .collect::<Vec<Framebuffer>>();

                    let width = i32::try_from(view.recommended_image_rect_width).unwrap();
                    let height = i32::try_from(view.recommended_image_rect_height).unwrap();
                    Swapchain {
                        width,
                        height,
                        view,
                        handle: RefCell::new(swapchain),
                        framebuffers: buffers,
                    }
                })
                .collect::<Vec<_>>();

            swapchain_handles
        });

        let (view_flags, views) = session
            .locate_views(VIEW_TYPE, xr_frame_state.predicted_display_time, &stage)
            .unwrap();

        // `should_render` being true does not promise the views are tracked:
        // for a frame or two after the session begins (donning the headset, or
        // waking it after it dozed) the runtime still reports the view pose as
        // invalid, and hands back a ZERO quaternion rather than a unit one.
        // Feeding that to the projection layer makes xrEndFrame fail with
        // ERROR_POSE_INVALID - which used to panic the render thread, and a
        // dead render thread stops draining NativeActivity's input queue, so
        // Horizon OS raises "shock2quest isn't responding" on the next key
        // event. Treat an untracked view exactly like a frame we were told not
        // to render: submit no layers and try again next frame.
        if !view_flags
            .contains(xr::ViewStateFlags::ORIENTATION_VALID | xr::ViewStateFlags::POSITION_VALID)
        {
            if untracked_view_frames == 0 {
                println!("SHOCK2QUEST_XR_VIEWS_UNTRACKED mission={mission} flags={view_flags:?}");
            }
            untracked_view_frames += 1;
            end_frame_with_no_layers(
                &mut frame_stream,
                xr_frame_state.predicted_display_time,
                environment_blend_mode,
            );
            if let Some(report) = frame_profiler.record_skipped(elapsed_time, update_elapsed) {
                print_frame_report(&mission, session_focused, report);
            }
            continue;
        }
        if untracked_view_frames > 0 {
            println!(
                "SHOCK2QUEST_XR_VIEWS_TRACKED mission={mission} untracked_frames={untracked_view_frames}"
            );
            untracked_view_frames = 0;
        }

        // Remember where the head actually is, for next frame's input context.
        if let Some(view) = views.first() {
            last_view_position = Some(stage_to_pawn(
                vec3(
                    view.pose.position.x,
                    view.pose.position.y,
                    view.pose.position.z,
                ),
                game.player_center_above_floor(),
            ));
            last_view_rotation = Some(cgmath::Quaternion::new(
                view.pose.orientation.w,
                view.pose.orientation.x,
                view.pose.orientation.y,
                view.pose.orientation.z,
            ));
        }

        let (_, _eyes) = session
            .locate_views(
                VIEW_TYPE,
                xr_frame_state.predicted_display_time,
                &head_space,
            )
            .unwrap();

        let scene_started = Instant::now();
        let (scene, camera_pos, camera_rot) = game.render();
        let scene_elapsed = scene_started.elapsed();

        // Render to each eye
        let time = now.elapsed().as_secs_f32();
        let (left_eye_elapsed, _) = render_swapchain(
            &mut game,
            &engine,
            camera_pos,
            camera_rot,
            &swapchain[0],
            time,
            &views[0],
            true,
            &scene,
            false,
        );
        let (right_eye_elapsed, finish_elapsed) = render_swapchain(
            &mut game,
            &engine,
            camera_pos,
            camera_rot,
            &swapchain[1],
            time,
            &views[1],
            true,
            &scene,
            true,
        );

        let swap1 = &swapchain[0].handle.borrow();
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                // TODO
                // width: view.resolution.width as _,
                // height: view.resolution.height as _,
                width: swapchain[0].width,
                height: swapchain[0].height,
            },
        };
        let sub1 = xr::SwapchainSubImage::new()
            .swapchain(swap1)
            .image_rect(rect);
        let swap2 = &swapchain[1].handle.borrow();
        let sub2 = xr::SwapchainSubImage::new()
            .swapchain(swap2)
            .image_rect(rect);
        let submit_started = Instant::now();
        // Never fatal: a rejected frame costs one dropped image, but a panic
        // here kills the render thread, and with it the per-frame drain of
        // NativeActivity's lifecycle/input queues - which is what turns a
        // one-frame compositor complaint into an app-wide ANR.
        if let Err(error) = frame_stream.end(
            xr_frame_state.predicted_display_time,
            environment_blend_mode,
            &[
                &xr::CompositionLayerProjection::new().space(&stage).views(&[
                    xr::CompositionLayerProjectionView::new()
                        .pose(views[0].pose)
                        .fov(views[0].fov)
                        .sub_image(sub1),
                    xr::CompositionLayerProjectionView::new()
                        .pose(views[1].pose)
                        .fov(views[1].fov)
                        .sub_image(sub2),
                ]),
            ],
        ) {
            // Only the start of a burst is logged, so a persistently unhappy
            // compositor cannot flood logcat at 90 Hz.
            if submit_failures == 0 {
                println!("SHOCK2QUEST_XR_SUBMIT_FAILED mission={mission} error={error:?}");
            }
            submit_failures += 1;
        } else {
            if submit_failures > 0 {
                println!(
                    "SHOCK2QUEST_XR_SUBMIT_RECOVERED mission={mission} failed_frames={submit_failures}"
                );
            }
            submit_failures = 0;
        }
        let submit_elapsed = submit_started.elapsed();

        let requested_refresh_is_active = display_refresh_rate.is_some_and(|active_hz| {
            requested_display_refresh_rate
                .is_none_or(|requested_hz| refresh_rate::rate_matches(active_hz, requested_hz))
        });
        if !ready_reported && requested_refresh_is_active {
            println!(
                "SHOCK2QUEST_READY mission={} target_refresh_hz={:.3} requested_refresh_hz={:.3} refresh_hz={:.3} eye_width={} eye_height={}",
                mission,
                refresh_rate::TARGET_HZ,
                requested_display_refresh_rate.unwrap_or_default(),
                display_refresh_rate.unwrap_or_default(),
                swapchain[0].width,
                swapchain[0].height
            );
            ready_reported = true;
        }

        if let Some(report) = frame_profiler.record(frame_profiler::FrameTimings {
            frame: elapsed_time,
            update: update_elapsed,
            scene: scene_elapsed,
            left_eye: left_eye_elapsed,
            right_eye: right_eye_elapsed,
            finish: finish_elapsed,
            submit: submit_elapsed,
        }) {
            print_frame_report(&mission, session_focused, report);
        }

        // let mut printed = false;
        // if right_aim.is_active(&session, xr::Path::NULL).unwrap() {
        //     print!(
        //         "Right Hand: ({:0<12},{:0<12},{:0<12})",
        //         right_aim_location.pose.position.x,
        //         right_aim_location.pose.position.y,
        //         right_aim_location.pose.position.z
        //     );
        //     printed = true;
        // }
        // if printed {
        //     println!();
        // }
        //render_time = Instant::now();
    }

    // The session is over (EXITING / LOSS_PENDING, or an instance loss). Just
    // returning is not enough on Android: ndk-glue runs `main` on a thread it
    // spawned, so the NativeActivity - and the process - would stay up with
    // nothing rendering, which the headset shows as a black void. Ask Android
    // to finish the activity, keep servicing its queues while it does, then end
    // the process.
    #[cfg(target_os = "android")]
    {
        println!("SHOCK2QUEST_XR_SHUTDOWN - finishing the activity");
        // ndk-glue 0.6 deprecates this in favor of `ndk_context`, which hands
        // back the JavaVM and the Java activity object - not the
        // `ANativeActivity` that `finish()` needs. This is still the only way
        // to reach ANativeActivity_finish from here.
        #[allow(deprecated)]
        ndk_glue::native_activity().finish();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline && !android_pump_events() {
            std::thread::sleep(Duration::from_millis(16));
        }
        // `process::exit` runs no destructors, so hand the XR objects back
        // explicitly first - otherwise even the clean exit destroys neither the
        // session nor the instance, and the runtime service can carry that
        // state into the next launch.
        drop(swapchain);
        drop(frame_stream);
        drop(frame_wait);
        drop(session);
        drop(xr_instance);
        std::process::exit(0);
    }

    // egl_display
    // let config = egl
    //     .choose_first_config(display, &attributes)
    //     .expect("unable to choose an EGL configuration")
    //     .expect("no EGL configuration found");

    // if vk_target_version_xr < reqs.min_api_version_supported
    //         || vk_target_version_xr.major() > reqs.max_api_version_supported.major()
    //     {
    //         panic!(
    //             "OpenXR runtime requires Vulkan version > {}, < {}.0.0",
    //             reqs.min_api_version_supported,
    //             reqs.max_api_version_supported.major() + 1
    //         );
    //     }
}

fn print_frame_report(mission: &str, focused: bool, report: frame_profiler::FrameReport) {
    println!(
        "SHOCK2QUEST_PERF mission={} focused={} samples={} skipped={} fps={:.3} frame_ms={:.3} update_ms={:.3} scene_ms={:.3} left_eye_ms={:.3} right_eye_ms={:.3} finish_ms={:.3} submit_ms={:.3}",
        mission,
        focused,
        report.frames,
        report.skipped_frames,
        report.fps,
        report.frame_ms,
        report.update_ms,
        report.scene_ms,
        report.left_eye_ms,
        report.right_eye_ms,
        report.finish_ms,
        report.submit_ms
    );
}

use cgmath::{Vector3, vec3};
use libm::*;
fn create_projection_matrix(fov: &xr::Fovf, near_z: f32, far_z: f32) -> cgmath::Matrix4<f32> {
    let tan_left = tanf(fov.angle_left);
    let tan_right = tanf(fov.angle_right);
    let tan_down = tanf(fov.angle_down);
    let tan_up = tanf(fov.angle_up);

    let tan_angle_width = tan_right - tan_left;

    // Set to tanAngleDown - tanAngleUp for a clip space with positive Y down (Vulkan).
    // Set to tanAngleUp - tanAngleDown for a clip space with positive Y up (OpenGL / D3D / Metal).
    let tan_height = tan_up - tan_down;

    // OpenGL / OpenGLES
    let offset_z = near_z;

    let c0r0 = 2.0 / tan_angle_width;
    let c0r1 = 0.0;
    let c0r2 = (tan_right + tan_left) / tan_angle_width;
    let c0r3 = 0.0;

    let c1r0 = 0.0;
    let c1r1 = 2.0 / tan_height;
    let c1r2 = (tan_up + tan_down) / tan_height;
    let c1r3 = 0.0;

    let c2r0 = 0.0;
    let c2r1 = 0.0;
    let c2r2 = -(far_z + offset_z) / (far_z - near_z);
    let c2r3 = -(far_z * (near_z + offset_z)) / (far_z - near_z);

    let c3r0 = 0.0;
    let c3r1 = 0.0;
    let c3r2 = -1.0;
    let c3r3 = 0.0;
    // cgmath::Matrix4::<f32>::new(
    //     c0r0, c0r1, c0r2, c0r3, c1r0, c1r1, c1r2, c1r3, c2r0, c2r1, c2r2, c2r3, c3r0, c3r1, c3r2,
    //     c3r3,
    // )
    cgmath::Matrix4::<f32>::new(
        c0r0, c1r0, c2r0, c3r0, c0r1, c1r1, c2r1, c3r1, c0r2, c1r2, c2r2, c3r2, c0r3, c1r3, c2r3,
        c3r3,
    )
}

/// Close out a frame that has nothing to show: submit no layers, so the runtime
/// composites nothing this frame (it does NOT hold the previous image). Used
/// both when the runtime tells us not to render and when the located views are
/// not yet tracked - in the latter case the alternative is a garbage pose, and
/// a blank frame or two while tracking comes up is the lesser evil.
///
/// Deliberately non-fatal for the same reason as the projection-layer submit:
/// a panic on the render thread stops the Android event pump and the app ANRs.
fn end_frame_with_no_layers(
    frame_stream: &mut xr::FrameStream<xr::OpenGlEs>,
    predicted_display_time: xr::Time,
    environment_blend_mode: xr::EnvironmentBlendMode,
) {
    if let Err(error) = frame_stream.end(predicted_display_time, environment_blend_mode, &[]) {
        println!("SHOCK2QUEST_XR_EMPTY_FRAME_FAILED error={error:?}");
    }
}

/// Non-blockingly drain the Android lifecycle and input queues once per frame.
///
/// This must go through the thread's `ALooper`: `ndk_glue::poll_events()` is a
/// raw read on the event pipe, and that pipe is left in blocking mode, so
/// calling it speculatively would park the render thread forever the moment
/// the queue ran dry. Polling the looper with a zero timeout first tells us
/// which queue actually has something pending, so every read below is
/// guaranteed not to block.
///
/// Returns whether the activity was destroyed during this drain. Teardown is
/// deliberately *not* driven from that during the main loop: the OpenXR session
/// state machine already exits it on EXITING/LOSS_PENDING after a clean
/// `session.end()`; breaking out on ndk-glue's `Destroy` instead would tear
/// down GL and XR objects after the activity is already gone, with the session
/// possibly never ended - strictly worse ordering. The flag is only read after
/// the loop, where it ends the post-`finish()` wait as soon as Android has
/// actually torn the activity down. `Destroy` also stops the drain for the frame.
///
/// The input events are finished as *unhandled* on purpose: gameplay input
/// arrives through OpenXR actions, and these only need to be consumed so the
/// watchdog sees the app servicing its queues. Reporting them unhandled lets
/// the platform apply its own default handling (e.g. BACK). `pre_dispatch`
/// must be honored - when it takes the event (IME and friends) it owns it, and
/// finishing it ourselves would be a double free.
#[cfg(target_os = "android")]
fn android_pump_events() -> bool {
    use ndk::looper::{Poll, ThreadLooper};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Upper bound on Android queue events serviced per frame, so a burst can
    /// never stall the 90 Hz render loop. Leftovers are handled next frame.
    const MAX_ANDROID_EVENTS_PER_FRAME: u32 = 32;

    static NO_LOOPER_REPORTED: AtomicBool = AtomicBool::new(false);

    let Some(looper) = ThreadLooper::for_thread() else {
        // No looper on this thread means the pump is inert and the ANR is
        // back - say so once rather than failing silently.
        if !NO_LOOPER_REPORTED.swap(true, Ordering::Relaxed) {
            println!(
                "android_pump_events: no ALooper for this thread - Android queues are NOT being serviced"
            );
        }
        return false;
    };

    // Bounded so a flooded queue can never starve rendering; anything left
    // over is picked up on the next frame. A single budget covers both the
    // outer poll and the inner input drain, so the cap is a real per-frame
    // event budget rather than just a poll count.
    let mut budget = MAX_ANDROID_EVENTS_PER_FRAME;
    while budget > 0 {
        budget -= 1;
        match looper.poll_all_timeout(Duration::ZERO) {
            Ok(Poll::Event { ident, .. }) => match ident {
                ndk_glue::NDK_GLUE_LOOPER_EVENT_PIPE_IDENT => {
                    if let Some(event) = ndk_glue::poll_events() {
                        // Rendering is already governed by the OpenXR session
                        // state, so the rest of the lifecycle is informational.
                        if event == ndk_glue::Event::Destroy {
                            println!("android_pump_events: activity destroyed");
                            return true;
                        }
                    }
                }
                ndk_glue::NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT => {
                    if let Some(input_queue) = ndk_glue::input_queue().as_ref() {
                        while budget > 0 {
                            let Some(event) = input_queue.get_event() else {
                                break;
                            };
                            budget -= 1;
                            if let Some(event) = input_queue.pre_dispatch(event) {
                                input_queue.finish_event(event, false);
                            }
                        }
                    }
                }
                _ => {}
            },
            // Both queues are empty (Timeout), we were only woken up (Wake),
            // or the looper ran a callback itself (Callback, which
            // `poll_all_timeout` never actually yields) - nothing more to
            // service this frame.
            Ok(Poll::Timeout) | Ok(Poll::Wake) | Ok(Poll::Callback) => break,
            Err(e) => {
                // A permanently failing looper would silently disable the
                // pump; keep it visible.
                println!("android_pump_events: looper poll failed: {e:?}");
                break;
            }
        }
    }
    false
}

/// Convert a floor-origin STAGE-space position (meters) into the game's pawn
/// space (world units, origin at the player collider's center): scale meters
/// to world units, then move the anchor from the physical floor up to the
/// collider center, so a tracked eye or hand N meters above the real floor
/// lands the equivalent height above the in-game floor. Without this the raw
/// meters were added to the collider CENTER unscaled, placing the standing
/// eye ~1.8 SS2 ft above the original game's eye line (and world scale ~31%
/// large).
fn stage_to_pawn(position_meters: Vector3<f32>, center_above_floor: f32) -> Vector3<f32> {
    // The dev-params eye-height offset raises or lowers the whole tracked
    // stage: every tracked position - head input, both hands, and the per-eye
    // view - routes through this one mapping, so they move together and the
    // hands never detach from the raised eye line. The per-eye cap in
    // `render_swapchain` applies after this, so an upward offset still cannot
    // push the view out of the collider crown.
    let stage_offset_meters = shock2vr::dev_params::get(shock2vr::dev_params::EYE_HEIGHT_OFFSET);
    (position_meters + vec3(0.0, stage_offset_meters, 0.0)) / shock2vr::METERS_PER_WORLD_UNIT
        - vec3(0.0, center_above_floor, 0.0)
}

fn render_swapchain(
    game: &mut App,
    engine: &Box<dyn engine::Engine>,
    camera_pos: Vector3<f32>,
    camera_rot: Quaternion<f32>,
    swapchain: &Swapchain,
    time: f32,
    view: &xr::View,
    _log: bool,
    scene: &Vec<SceneObject>,
    is_last: bool,
) -> (Duration, Duration) {
    let eye_started = Instant::now();
    let mut xr_swapchain = swapchain.handle.borrow_mut();
    let image_index1 = xr_swapchain.acquire_image().unwrap();
    // Wait until the image is available to render to. The compositor could still be
    // reading from it.
    xr_swapchain.wait_image(xr::Duration::INFINITE).unwrap();

    let framebuffer = swapchain.framebuffers.get(image_index1 as usize).unwrap();

    let width = swapchain.width;
    let height = swapchain.height;

    let mut head_offset = stage_to_pawn(
        cgmath::Vector3::new(
            view.pose.position.x,
            view.pose.position.y,
            view.pose.position.z,
        ),
        game.player_center_above_floor(),
    );
    // The tracked eye belongs to a real body, not to the game capsule, so it
    // must be held inside the collider crown: a physically crouched adult's
    // eye sits well above the short crouched capsule, and uncapped the player
    // sees over and through the very geometry the capsule clears (looking out
    // of the world from inside a duct). Only the view is capped - hand poses
    // have no such clipping concern and clamping them would break reaching up.
    // The cap binds essentially only while crouched (or button-latched);
    // standing it sits above any realistic head, so tracking stays 1:1.
    head_offset.y = head_offset.y.min(game.player_eye_cap_above_center());
    let head_rotation = cgmath::Quaternion::new(
        view.pose.orientation.w,
        view.pose.orientation.x,
        view.pose.orientation.y,
        view.pose.orientation.z,
    );
    let projection_matrix = create_projection_matrix(&view.fov, 0.1, 1000.);
    let screen_size = vec2(width as f32, height as f32);
    let render_context = engine::EngineRenderContext {
        time,
        camera_offset: camera_pos,
        camera_rotation: camera_rot,

        head_offset,
        head_rotation,

        projection_matrix,

        screen_size,
    };

    let view_matrix = engine::util::compute_view_matrix_from_render_context(&render_context);
    let mut all_scene_objs = game.render_per_eye(view_matrix, projection_matrix, screen_size);
    all_scene_objs.extend(scene.iter().cloned());

    // SAFETY: This is only calling into OpenGL APIs and not copying memory
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, framebuffer.gl_color_buffer);
        // Render
        gl::Viewport(0, 0, width, height);
        gl::Scissor(0, 0, width, height);
        // gl::ClearColor(r, g, b, 1.0);
        // gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);

        let mut scene_for_render = Scene::from_objects(all_scene_objs);

        // Add hand spotlights for enhanced lighting testing (experimental feature)
        let hand_spotlights = game.get_hand_spotlights();
        for spotlight in hand_spotlights {
            scene_for_render.lights_mut().add_spotlight(spotlight);
        }

        profile!(
            "[oculus.engine.render]",
            engine.render(&render_context, &scene_for_render)
        );

        // GL(glViewport(0, 0, frameBuffer->Width, frameBuffer->Height));
        // GL(glScissor(0, 0, frameBuffer->Width, frameBuffer->Height));
        // GL(glClearColor(1.0f, 0.0f, 0.0f, clearAlpha));
        // GL(glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT));

        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
    }

    let finish_elapsed = if is_last {
        let finish_started = Instant::now();
        game.finish_render(view_matrix, projection_matrix, screen_size);
        finish_started.elapsed()
    } else {
        Duration::ZERO
    };

    xr_swapchain.release_image().unwrap();

    (
        eye_started.elapsed().saturating_sub(finish_elapsed),
        finish_elapsed,
    )
}

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
pub const VIEW_COUNT: u32 = 2;

//#[derive(Debug)]
#[allow(dead_code)]
struct Swapchain {
    width: i32,
    height: i32,
    view: xr::ViewConfigurationView,
    handle: RefCell<xr::Swapchain<xr::OpenGlEs>>,
    framebuffers: Vec<Framebuffer>,
    //     buffers: Vec<Framebuffer>,
    //     resolution: vk::Extent2D,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct Framebuffer {
    image: u32,
    depth_buffer: gl::types::GLuint,
    gl_color_buffer: gl::types::GLuint,
}
