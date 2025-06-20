extern crate gl;
extern crate khronos_egl as egl;

use cgmath::vec2;
use cgmath::Quaternion;
use dark::mission;
use dark::mission::TextureSize;
use engine::profile;
use engine::scene::Scene;
use engine::scene::SceneObject;
use openxr as xr;
use shock2vr::input_context::InputContext;
use shock2vr::Game;
use shock2vr::GameOptions;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::rc::Rc;

use ndk::asset::{Asset, AssetManager};

mod android_permissions;

use tokio::runtime::Runtime;

// Fix to get c++ shared (for __cxa_pure_virtual issue):
// - https://github.com/RustAudio/cpal/issues/563
// - https://github.com/rust-mobile/cargo-apk/issues/13
#[cfg_attr(target_os = "android", link(name = "c++_shared"))]
extern "C" {}

#[cfg_attr(target_os = "android", ndk_glue::main)]
fn main() {
    #[cfg(feature = "linked")]
    let entry = xr::Entry::linked();
    #[cfg(not(feature = "linked"))]
    let entry = xr::Entry::load()
        .expect("couldn't find the OpenXR loader; try enabling the \"static\" feature");

    #[cfg(target_os = "android")]
    entry.initialize_android_loader();
    let mut rt = Runtime::new().unwrap();
    rt.block_on(async move {
        println!("hello from the async block");
        tokio::spawn(async { android_permissions::request_permission().await });

        //bonus, you could spawn tasks too
        // tokio::spawn(async { async_function("task1").await });

        // tokio::spawn(async { async_function("task2").await });
    });
    println!(
        "after async: {}",
        env::current_dir().unwrap().to_str().unwrap()
    );

    //std::fs::create_dir("/mnt/sdcard/shock2quest").unwrap();

    let paths = std::fs::read_dir("/mnt/sdcard/shock2quest/res/obj/txt16").unwrap();
    for path in paths {
        println!("Name: {}", path.unwrap().path().display())
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
        .graphics_requirements::<xr::OpenGLES>(system)
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
    let attributes = [
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

    egl.get_configs(egl_display, &mut configs);
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
    let tinySurface = egl
        .create_pbuffer_surface(egl_display, config, &surface_attributes)
        .unwrap();
    println!("Created surface!");

    egl.make_current(
        egl_display,
        Some(tinySurface),
        Some(tinySurface),
        Some(context),
    )
    .unwrap();

    unsafe {
        let mut majorVersion = 0;
        let mut minorVersion = 0;
        gl::load_with(|s| match egl.get_proc_address(s) {
            None => 0 as *const _,
            Some(v) => v as *const _,
        });
        gl::GetIntegerv(gl::MAJOR_VERSION, &mut majorVersion);
        gl::GetIntegerv(gl::MINOR_VERSION, &mut minorVersion);
        println!("Major: {} Minor: {}", majorVersion, minorVersion);
    }

    let systemId = xr_instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .unwrap();
    println!("System ID: {:?}", systemId);
    // A session represents this application's desire to display things! This is where we hook
    // up our graphics API. This does not start the session; for that, you'll need a call to
    // Session::begin, which we do in 'main_loop below.
    let sessionCreateInfo = unsafe {
        &xr::opengles::SessionCreateInfo::Android {
            context: context.as_ptr(),
            display: egl_display.as_ptr(),
            config: config.as_ptr(),
        }
    };
    let (session, mut frame_wait, mut frame_stream) = unsafe {
        xr_instance
            .create_session::<xr::OpenGLES>(system, sessionCreateInfo)
            .unwrap()
    };

    // Create a stage!
    let stage = session
        .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
        .unwrap();

    let head_space = session
        .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
        .unwrap();

    let local_space = session
        .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
        .unwrap();

    let right_hand_path = xr_instance.string_to_path("/user/hand/right").unwrap();
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
            ],
        )
        .unwrap();

    // Attach the action set to the session
    session.attach_action_sets(&[&action_set]).unwrap();

    // Create an action space for each device we want to locate
    let right_aim_space = right_aim
        .create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();

    let left_aim_space = left_aim
        .create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();

    // Main loop
    let mut swapchain = None;
    let mut event_storage = xr::EventDataBuffer::new();
    let mut session_running = false;
    // Index of the current frame, wrapped by PIPELINE_DEPTH. Not to be confused with the
    // swapchain image index.
    let mut frame = 0;
    let now = Instant::now();
    let engine = engine::android();
    let file_system = engine.get_storage().external_filesystem();
    let mut experimental_features = HashSet::new();
    experimental_features.insert("gui".to_owned());
    let options: GameOptions = GameOptions {
        render_particles: false,
        mission: "medsci2.mis".to_string(),
        experimental_features,
        ..GameOptions::default()
    };
    let mut game = shock2vr::Game::init(&file_system, options);

    let mut camera_pos = vec3(0.0, 5.0, 10.0);

    let mut render_time = Instant::now();
    let mut last_update_time = render_time;
    'main_loop: loop {
        frame = frame + 1;

        println!("Starting frame: {}", frame);
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
                    match e.state() {
                        xr::SessionState::READY => {
                            session.begin(VIEW_TYPE).unwrap();
                            session_running = true;
                            let refresh_rate = session.get_display_refresh_rate().unwrap();

                            // let available_rates =
                            //     session.enumerate_display_refresh_rates().unwrap();
                            // println!("refresh rate: {}", refresh_rate);
                            // println!("all rates: {:?}", available_rates);
                            // //panic!("refresh rate");
                            // session.request_display_refresh_rate(90.0).unwrap();
                        }
                        xr::SessionState::STOPPING => {
                            session.end().unwrap();
                            session_running = false;
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
                EventsLost(e) => {
                    println!("lost {} events", e.lost_event_count());
                }
                _ => {}
            }
        }
        if !session_running {
            // Don't grind up the CPU
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        // println!(
        //     " - After polling events: {}",
        //     render_time.elapsed().as_secs_f32()
        // );

        // Block until the previous frame is finished displaying, and is ready for another one.
        // Also returns a prediction of when the next frame will be displayed, for use with
        // predicting locations of controllers, viewpoints, etc.
        let xr_frame_state = frame_wait.wait().unwrap();

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

        let left_thumbstick_value = left_thumbstick_action
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;
        let right_thumbstick_value = right_thumbstick_action
            .state(&session, xr::Path::NULL)
            .unwrap()
            .current_state;

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

        let speed = 50.0;

        // let forward_xr = right_aim_location.pose.orientation;
        // //let forward_xr = views[0].pose.orientation;
        // let dir = cgmath::Quaternion::new(forward_xr.w, forward_xr.x, forward_xr.y, forward_xr.z);

        // let forward = dir.rotate_vector(vec3(0.0, 0.0, -time * right_trigger_value * 10.));
        let head_rotation = cgmath::Quaternion::new(
            right_aim_location.pose.orientation.w,
            right_aim_location.pose.orientation.x,
            right_aim_location.pose.orientation.y,
            right_aim_location.pose.orientation.z,
        );

        let right_hand_position = vec3(
            right_aim_location.pose.position.x,
            right_aim_location.pose.position.y,
            right_aim_location.pose.position.z,
        );

        let left_hand_position = vec3(
            left_aim_location.pose.position.x,
            left_aim_location.pose.position.y,
            left_aim_location.pose.position.z,
        );
        let left_hand_rotation = cgmath::Quaternion::new(
            left_aim_location.pose.orientation.w,
            left_aim_location.pose.orientation.x,
            left_aim_location.pose.orientation.y,
            left_aim_location.pose.orientation.z,
        );

        let mut input_context = InputContext::default();
        input_context.head.rotation = head_rotation;
        input_context.right_hand.rotation = head_rotation;
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
        game.update(&time_context, &input_context, vec![]);

        // Must be called before any rendering is done!
        frame_stream.begin().unwrap();

        // println!(
        //     " - After blocking for previous frame: {}",
        //     render_time.elapsed().as_secs_f32()
        // );

        if !xr_frame_state.should_render {
            //println!("Skipping frame!");
            frame_stream
                .end(
                    xr_frame_state.predicted_display_time,
                    environment_blend_mode,
                    &[],
                )
                .unwrap();
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
                                let result = gl::CheckFramebufferStatus(gl::DRAW_FRAMEBUFFER);
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

        let (_, views) = session
            .locate_views(VIEW_TYPE, xr_frame_state.predicted_display_time, &stage)
            .unwrap();

        let (_, eyes) = session
            .locate_views(
                VIEW_TYPE,
                xr_frame_state.predicted_display_time,
                &head_space,
            )
            .unwrap();

        let (scene, camera_pos, camera_rot) = game.render();

        // Render to each eye
        let time = now.elapsed().as_secs_f32();
        render_swapchain(
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
        render_swapchain(
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
        frame_stream
            .end(
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
            )
            .unwrap();

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

use cgmath::{prelude::*, vec3, Vector3};
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

fn render_swapchain(
    game: &mut Game,
    engine: &Box<dyn engine::Engine>,
    camera_pos: Vector3<f32>,
    camera_rot: Quaternion<f32>,
    swapchain: &Swapchain,
    time: f32,
    view: &xr::View,
    log: bool,
    scene: &Vec<SceneObject>,
    is_last: bool,
) -> () {
    let mut xrSwapchain = swapchain.handle.borrow_mut();
    let image_index1 = xrSwapchain.acquire_image().unwrap();
    // Wait until the image is available to render to. The compositor could still be
    // reading from it.
    xrSwapchain.wait_image(xr::Duration::INFINITE).unwrap();

    let framebuffer = swapchain.framebuffers.get(image_index1 as usize).unwrap();

    let width = swapchain.width;
    let height = swapchain.height;

    let head_offset = cgmath::Vector3::new(
        view.pose.position.x,
        view.pose.position.y,
        view.pose.position.z,
        //0.0, 0.0, 5.0,
    );
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

        profile!(
            "[oculus.engine.render]",
            engine.render(&render_context, &all_scene_objs)
        );

        // GL(glViewport(0, 0, frameBuffer->Width, frameBuffer->Height));
        // GL(glScissor(0, 0, frameBuffer->Width, frameBuffer->Height));
        // GL(glClearColor(1.0f, 0.0f, 0.0f, clearAlpha));
        // GL(glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT));

        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
    }

    if is_last {
        game.finish_render(view_matrix, projection_matrix, screen_size);
    }

    println!("-- Finished rendering");
    xrSwapchain.release_image().unwrap();
}

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
pub const VIEW_COUNT: u32 = 2;

//#[derive(Debug)]
struct Swapchain {
    width: i32,
    height: i32,
    view: xr::ViewConfigurationView,
    handle: RefCell<xr::Swapchain<xr::OpenGLES>>,
    framebuffers: Vec<Framebuffer>,
    //     buffers: Vec<Framebuffer>,
    //     resolution: vk::Extent2D,
}

#[derive(Clone, Copy)]
struct Framebuffer {
    image: u32,
    depth_buffer: gl::types::GLuint,
    gl_color_buffer: gl::types::GLuint,
}
