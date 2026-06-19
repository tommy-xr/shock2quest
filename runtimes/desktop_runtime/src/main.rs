extern crate glfw;
use self::glfw::{Action, Context, Key};
use cgmath::Decomposed;
use cgmath::point3;

use clap::Parser;

use dark::SCALE_FACTOR;
use engine::profile;
use engine::scene::Scene;

use engine::util::compute_view_matrix_from_render_context;
use glfw::GlfwReceiver;
use glfw::Modifiers;

use shock2vr::GameOptions;
use shock2vr::SpawnLocation;
use tracing::trace;

extern crate gl;

use cgmath::prelude::*;
use cgmath::vec2;
use cgmath::{Quaternion, Vector3, vec3};

use glfw::MouseButton;
use shock2vr::input::InputActionState;
use shock2vr::input_context::InputContext;

mod input_mapper;
use input_mapper::DesktopInputMapper;
use shock2vr::time::Time;
use std::collections::HashSet;
use std::time::Duration;

// settings
const SCR_WIDTH: u32 = 800;
const SCR_HEIGHT: u32 = 600;

struct MousePosition {
    x: f32,
    y: f32,
}

struct CameraContext {
    pitch: f32,
    yaw: f32,
    mouse_position: Option<MousePosition>,
}

impl CameraContext {
    pub fn new() -> CameraContext {
        CameraContext {
            pitch: 0.0,
            yaw: 0.0,
            mouse_position: None,
        }
    }
}
struct HandContext {
    left_hand_context: CameraContext,
    right_hand_context: CameraContext,

    right_trigger_pressed: bool,
    right_squeeze_pressed: bool,
    right_a_pressed: bool,

    left_trigger_pressed: bool,
    left_squeeze_pressed: bool,
    left_a_pressed: bool,
}

impl HandContext {
    pub fn new() -> HandContext {
        HandContext {
            left_hand_context: CameraContext::new(),
            right_hand_context: CameraContext::new(),

            right_trigger_pressed: false,
            right_squeeze_pressed: false,
            right_a_pressed: false,

            left_squeeze_pressed: false,
            left_trigger_pressed: false,
            left_a_pressed: false,
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Mission or debug scene to load. If omitted, boots the main menu in flat
    /// mode, or earth.mis in --vr mode.
    #[arg(short, long)]
    mission: Option<String>,

    #[arg(long = "debug-physics")]
    debug_physics: bool,

    #[arg(long = "debug-portals")]
    debug_portals: bool,

    #[arg(long = "debug-draw")]
    debug_draw: bool,

    #[arg(long = "debug-show-ids")]
    debug_show_ids: bool,

    #[arg(long = "debug-skeletons")]
    debug_skeletons: bool,

    #[arg(long = "debug-ai")]
    debug_ai: bool,

    #[arg(long = "debug-pathfinding")]
    debug_pathfinding: bool,

    #[arg(short, long, default_value = None)]
    save_file: Option<String>,
    // Number of times to greet
    // #[arg(short, long, default_value_t = 1)]
    // count: u8,
    #[arg(short, long, default_value = None)]
    experimental: Option<Vec<String>>,

    /// Run the emulated VR rig (mouse-driven hands, forearm HUD panels). The
    /// default presentation is flatscreen (screen-space 2D HUD, clean
    /// first-person view).
    #[arg(long)]
    vr: bool,
}
struct MouseUpdateResult {
    delta_x: f32,
    delta_y: f32,
}

fn camera_update_mouse(camera: &mut CameraContext, x_pos: f32, y_pos: f32) -> MouseUpdateResult {
    match camera.mouse_position {
        None => {
            camera.mouse_position = Some(MousePosition { x: x_pos, y: y_pos });
            MouseUpdateResult {
                delta_x: 0.0,
                delta_y: 0.0,
            }
        }
        Some(MousePosition { x, y }) => {
            let delta_x = x_pos - x;
            let delta_y = y_pos - y;
            camera.mouse_position = Some(MousePosition { x: x_pos, y: y_pos });
            MouseUpdateResult { delta_x, delta_y }
        }
    }
}

fn camera_forward(camera: &CameraContext) -> Vector3<f32> {
    let yaw = camera.yaw;
    let pitch = camera.pitch;
    let front = Vector3 {
        x: yaw.to_radians().cos() * pitch.to_radians().cos(),
        y: pitch.to_radians().sin(),
        z: yaw.to_radians().sin() * pitch.to_radians().cos(),
    };

    front.normalize()
}

fn camera_rotation(camera: &CameraContext) -> Quaternion<f32> {
    let forward = camera_forward(camera);
    let forward_p = cgmath::Point3::new(forward.x, forward.y, forward.z);

    let up: Vector3<f32> = vec3(0.0, 1.0, 0.0);
    let mat: Decomposed<Vector3<f32>, Quaternion<f32>> =
        cgmath::Transform::look_at_rh(forward_p, point3(0.0, 0.0, 0.0), up);
    mat.rot.invert()
}

fn f32_from_bool(v: bool) -> f32 {
    if v { 1.0 } else { 0.0 }
}

pub enum Mode {
    Gameplay,
    Editor,
}

pub enum Effect {
    SwitchToEditorMode,
    SwitchToGameplayMode,
}

pub fn main() {
    // glfw: initialize and configure
    // ------------------------------

    //tracing_subscriber::fmt::init();
    let args = Args::parse();
    //panic!("args: {:?}", args);
    let mut glfw = glfw::init(glfw::fail_on_errors).unwrap();
    // TODO: Figure out ANGLE
    // glfw.window_hint(glfw::WindowHint::ClientApi(glfw::OpenGlEs));
    glfw.window_hint(glfw::WindowHint::ContextVersion(4, 1));
    glfw.window_hint(glfw::WindowHint::OpenGlProfile(
        glfw::OpenGlProfileHint::Core,
    ));
    #[cfg(target_os = "macos")]
    glfw.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));

    // glfw window creation
    // --------------------
    let (mut window, events) = glfw
        .create_window(
            SCR_WIDTH,
            SCR_HEIGHT,
            "Shock Engine - Game Mode",
            glfw::WindowMode::Windowed,
        )
        .expect("Failed to create GLFW window");

    window.make_current();
    window.set_key_polling(true);
    window.set_cursor_pos_polling(true);
    window.set_framebuffer_size_polling(true);
    window.set_cursor_mode(glfw::CursorMode::Disabled);

    // gl: load all OpenGL function pointers
    // ---------------------------------------
    gl::load_with(|symbol| window.get_proc_address(symbol) as *const _);

    println!(
        "es2 extension supported: {}",
        glfw.extension_supported("GL_ARB_ES2_compatibility")
    );
    println!(
        "es3 extension supported: {}",
        glfw.extension_supported("GL_ARB_ES3_compatibility")
    );

    let engine = engine::opengl();
    let bundle_storage = engine.get_storage();
    let experimental_features: HashSet<String> =
        args.experimental.unwrap_or(vec![]).into_iter().collect();

    // Flatscreen is the default desktop presentation; --vr opts into the
    // emulated VR rig.
    let presentation_mode = if args.vr {
        shock2vr::PresentationMode::Vr
    } else {
        shock2vr::PresentationMode::Flat
    };

    // With no explicit mission, flat boots the (mouse-driven) main menu; the VR
    // rig has no pointer, so it boots straight into the first mission.
    let mission_arg = args.mission.clone().unwrap_or_else(|| {
        if args.vr {
            "earth.mis".to_owned()
        } else {
            "main_menu".to_owned()
        }
    });
    let (mission, spawn_location) = parse_mission(&mission_arg);

    let options = GameOptions {
        mission,
        presentation_mode,
        spawn_location,
        save_file: args.save_file,
        debug_draw: args.debug_draw,
        debug_physics: args.debug_physics,
        debug_portals: args.debug_portals,
        debug_show_ids: args.debug_show_ids,
        debug_skeletons: args.debug_skeletons,
        debug_ai: args.debug_ai,
        debug_pathfinding: args.debug_pathfinding,
        render_particles: true,
        experimental_features,
        ..GameOptions::default()
    };
    let mut game = shock2vr::Game::init(options, bundle_storage);
    // FOR SCREENSHOT
    // let mut camera_context = CameraContext {
    //     camera_offset: cgmath::Vector3::new(1.25, -14.0, -24.0),
    //     pitch: 4.81,
    //     yaw: -213.0,
    //     mouse_position: None,
    // };

    let mut camera_context = CameraContext::new();

    let mut hand_context = HandContext::new();

    let mut last_time = glfw.get_time() as f32;
    let start_time = last_time;

    let mut _frame = 0;
    let mut input_mapper = DesktopInputMapper::new();

    // Tracks whether the OS cursor is currently visible (Normal) vs captured
    // for mouse-look (Disabled). The window starts with the cursor disabled.
    let mut cursor_visible = false;
    let mut action_state = InputActionState::new();

    let _mode = Mode::Gameplay;
    // render loop
    // -----------
    while !window.should_close() {
        // events
        // -----
        let time = glfw.get_time() as f32;
        let delta_time = time - last_time;
        last_time = time;

        // A scene (e.g. the main menu) may want a visible 2D cursor instead of
        // captured mouse-look. Toggle the OS cursor when that changes.
        let wants_pointer = game.wants_pointer();
        if wants_pointer != cursor_visible {
            window.set_cursor_mode(if wants_pointer {
                glfw::CursorMode::Normal
            } else {
                glfw::CursorMode::Disabled
            });
            cursor_visible = wants_pointer;
        }

        let (mut input_context, input_state) = process_events(
            &mut window,
            &mut camera_context,
            &mut hand_context,
            &events,
            delta_time,
            &mut input_mapper,
            &mut action_state,
            wants_pointer,
        );

        // Flat first-person: left mouse button fires the wielded weapon; right
        // mouse button - or shift+left-click, which is easier on a trackpad -
        // uses/frobs/picks up. (The flat controller reads the right-hand trigger
        // + squeeze.)
        if !args.vr {
            let pressed = |b| window.get_mouse_button(b) == Action::Press;
            let shift = window.get_key(Key::LeftShift) == Action::Press
                || window.get_key(Key::RightShift) == Action::Press;
            let lmb = pressed(MouseButton::Button1);
            input_context.right_hand.trigger_value = if lmb && !shift { 1.0 } else { 0.0 };
            input_context.right_hand.squeeze_value =
                if pressed(MouseButton::Button2) || (lmb && shift) {
                    1.0
                } else {
                    0.0
                };
        }

        let ratio = SCR_WIDTH as f32 / SCR_HEIGHT as f32;
        let projection_matrix: cgmath::Matrix4<f32> =
            cgmath::perspective(cgmath::Deg(45.0), ratio, 0.1, 1000.0);

        let time = Time {
            elapsed: Duration::from_secs_f32(delta_time),
            total: Duration::from_secs_f32(time - start_time),
        };

        profile!(
            "game.update",
            game.update(&time, &input_context, &mut action_state)
        );

        // A scene may request quitting (e.g. the main menu's Quit item).
        if game.should_quit() {
            window.set_should_close(true);
        }

        let screen_size = vec2(SCR_WIDTH as f32, SCR_HEIGHT as f32);

        let (mut scene, pawn_offset, pawn_rotation) = profile!("game.render", game.render());

        let head_height = if input_state.is_crouching { 1.5 } else { 4.0 };
        let render_context = engine::EngineRenderContext {
            time: glfw.get_time() as f32,
            camera_offset: pawn_offset,
            camera_rotation: pawn_rotation,

            head_offset: vec3(0.0, head_height / SCALE_FACTOR, 0.0),
            head_rotation: camera_rotation(&camera_context),

            projection_matrix,
            screen_size,
        };

        let view = compute_view_matrix_from_render_context(&render_context);
        let per_eye_scene = profile!(
            "game.render_per_eye",
            game.render_per_eye(view, projection_matrix, screen_size)
        );

        game.finish_render(view, projection_matrix, screen_size);

        _frame += 1;

        scene.extend(per_eye_scene);

        let mut scene_for_render = Scene::from_objects(scene);

        // Add hand spotlights for enhanced lighting testing (experimental feature)
        let hand_spotlights = game.get_hand_spotlights();
        for spotlight in hand_spotlights {
            scene_for_render.lights_mut().add_spotlight(spotlight);
        }

        profile!(
            "engine.render",
            engine.render(&render_context, &scene_for_render)
        );

        // glfw: swap buffers and poll IO events (keys pressed/released, mouse moved etc.)
        // -------------------------------------------------------------------------------
        window.swap_buffers();
        glfw.poll_events();
    }
}

fn parse_mission(mission: &str) -> (String, SpawnLocation) {
    if !mission.contains(':') {
        return (mission.to_owned(), SpawnLocation::MapDefault);
    }

    let parts: Vec<&str> = mission.split(':').collect();

    if parts.len() > 2 {
        panic!("Unable to parse mission argument: {}", mission);
    }

    let mission = parts[0];

    let spawn_location = if parts[1].contains(",") {
        let vec_parts: Vec<&str> = parts[1].split(',').collect();
        if vec_parts.len() != 3 {
            panic!("Unable to parse position: {}", parts[1]);
        }

        let x = vec_parts[0].parse::<f32>().unwrap();
        let y = vec_parts[1].parse::<f32>().unwrap();
        let z = vec_parts[2].parse::<f32>().unwrap();
        SpawnLocation::PositionRotation(vec3(x, y, z), Quaternion::<f32>::new(1.0, 0.0, 0.0, 0.0))
    } else {
        match parts[1].parse::<i32>() {
            Ok(num) => SpawnLocation::Marker(num),
            Err(_) => SpawnLocation::MapDefault,
        }
    };

    return (mission.to_owned(), spawn_location);
}

struct InputState {
    is_crouching: bool,
}
impl InputState {
    pub fn new() -> Self {
        Self {
            is_crouching: false,
        }
    }
}

// NOTE: not the same version as in common.rs!
fn process_events(
    //audio: &mut AudioContext,
    window: &mut glfw::Window,
    camera_context: &mut CameraContext,
    hand_context: &mut HandContext,
    events: &GlfwReceiver<(f64, glfw::WindowEvent)>,
    delta_time: f32,
    input_mapper: &mut DesktopInputMapper,
    action_state: &mut InputActionState,
    wants_pointer: bool,
) -> (InputContext, InputState) {
    let _speed = 20.0;
    let head_rot_speed = 10.0;

    let _movement = cgmath::Vector3::new(0.0, 0.0, 0.0);

    trace!("delta time: {delta_time}");
    let mut rot_yaw = 0.0;
    let mut rot_pitch = 0.0;
    // if window.get_key(Key::Left) == Action::Press {
    //     println!("left key pressed");
    //     rot_yaw += 1.0;
    // }

    // if window.get_key(Key::Right) == Action::Press {
    //     rot_yaw += -1.0;
    // }

    for (_, event) in glfw::flush_messages(events) {
        match event {
            glfw::WindowEvent::FramebufferSize(width, height) => {
                // make sure the viewport matches the new window dimensions; note that width and
                // height will be significantly larger than specified on retina displays.
                unsafe { gl::Viewport(0, 0, width, height) }
            }
            glfw::WindowEvent::Key(Key::E, _, Action::Press, Modifiers::Alt) => {
                window.set_cursor_mode(glfw::CursorMode::Normal);
                //effects.push(Effect::SwitchToEditorMode)
            }
            glfw::WindowEvent::Key(Key::G, _, Action::Press, Modifiers::Alt) => {
                window.set_cursor_mode(glfw::CursorMode::Disabled);
                //effects.push(Effect::SwitchToGameplayMode)
            }
            // glfw::WindowEvent::Key(Key::Space, _, Action::Press, _) => {
            //     engine::audio::play_audio(audio)
            // }
            glfw::WindowEvent::Key(Key::Escape, _, Action::Press, _) => {
                window.set_should_close(true)
            }
            glfw::WindowEvent::CursorPos(x, y) => {
                let mouse_update = camera_update_mouse(camera_context, x as f32, y as f32);
                rot_yaw = 1.0 * mouse_update.delta_x;
                rot_pitch = 1.0 * mouse_update.delta_y;
            }
            _ => {}
        }
    }

    if window.get_key(Key::E) == Action::Press {
        hand_context.right_hand_context.yaw += rot_yaw * head_rot_speed * delta_time;
        hand_context.right_hand_context.pitch += rot_pitch * head_rot_speed * delta_time;

        hand_context.right_trigger_pressed =
            window.get_mouse_button(MouseButton::Button1) == Action::Press;
        hand_context.right_squeeze_pressed =
            window.get_mouse_button(MouseButton::Button2) == Action::Press;
        hand_context.right_a_pressed =
            window.get_mouse_button(MouseButton::Button3) == Action::Press;
    } else if window.get_key(Key::Q) == Action::Press {
        hand_context.left_hand_context.yaw += rot_yaw * head_rot_speed * delta_time;
        hand_context.left_hand_context.pitch += rot_pitch * head_rot_speed * delta_time;

        hand_context.left_trigger_pressed =
            window.get_mouse_button(MouseButton::Button1) == Action::Press;
        hand_context.left_squeeze_pressed =
            window.get_mouse_button(MouseButton::Button2) == Action::Press;
        hand_context.left_a_pressed =
            window.get_mouse_button(MouseButton::Button3) == Action::Press;
    } else {
        camera_context.yaw += rot_yaw * head_rot_speed * delta_time;
        camera_context.pitch += rot_pitch * head_rot_speed * delta_time;

        hand_context.left_hand_context.yaw = 90.0;
        hand_context.left_hand_context.pitch = 0.0;

        hand_context.right_hand_context.yaw = 90.0;
        hand_context.right_hand_context.pitch = 0.0;
    }

    if camera_context.pitch < -89.0 {
        camera_context.pitch = -89.0
    }

    if camera_context.pitch > 89.0 {
        camera_context.pitch = 89.0
    }

    //let rotation = camera_rotation(camera_context);
    let forward = -1.0 * camera_forward(camera_context);
    let right = Vector3::cross(forward, vec3(0.0, 1.0, 0.0)).normalize();

    let mut right_thumbstick_value = vec2(0.0, 0.0);
    let mut left_thumbstick_value = vec2(0.0, 0.0);
    //let mut trigger_value = 0.0;

    let is_alt_pressed = window.get_key(Key::LeftAlt) == Action::Press
        || window.get_key(Key::RightAlt) == Action::Press;

    if window.get_key(Key::W) == Action::Press && !is_alt_pressed {
        right_thumbstick_value += vec2(0.0, 1.0);
    }

    if window.get_key(Key::S) == Action::Press && !is_alt_pressed {
        right_thumbstick_value += vec2(0.0, -1.0);
    }

    if window.get_key(Key::A) == Action::Press && !is_alt_pressed {
        right_thumbstick_value += vec2(1.0, 0.0);
    }

    if window.get_key(Key::D) == Action::Press && !is_alt_pressed {
        right_thumbstick_value += vec2(-1.0, 0.0);
    }

    if window.get_key(Key::Up) == Action::Press {
        left_thumbstick_value += vec2(0.0, 1.0);
    }

    if window.get_key(Key::Down) == Action::Press {
        left_thumbstick_value += vec2(0.0, -1.0);
    }

    if window.get_key(Key::Left) == Action::Press {
        left_thumbstick_value += vec2(1.0, 0.0);
    }

    if window.get_key(Key::Right) == Action::Press {
        left_thumbstick_value += vec2(-1.0, 0.0);
    }

    if window.get_key(Key::LeftShift) == Action::Press
        || window.get_key(Key::RightShift) == Action::Press
    {
        left_thumbstick_value *= 2.0;
        right_thumbstick_value *= 2.0;
    }

    let mut input_context = InputContext::default();
    let head_rotation = camera_rotation(camera_context);
    input_context.head.rotation = head_rotation;
    input_context.right_hand.position = vec3(0.0, 2.0 / SCALE_FACTOR, 0.0)
        + right * 2.0 / SCALE_FACTOR
        + (forward * 4.0 / SCALE_FACTOR);
    input_context.right_hand.rotation =
        head_rotation * camera_rotation(&hand_context.right_hand_context);
    input_context.right_hand.thumbstick = right_thumbstick_value;
    input_context.right_hand.trigger_value = f32_from_bool(hand_context.right_trigger_pressed);
    input_context.right_hand.squeeze_value = f32_from_bool(hand_context.right_squeeze_pressed);
    input_context.right_hand.a_value = f32_from_bool(hand_context.right_a_pressed);

    input_context.left_hand.position = vec3(0.0, 2.0 / SCALE_FACTOR, 0.0)
        - right * 2.0 / SCALE_FACTOR
        + (forward * 4.0 / SCALE_FACTOR);
    input_context.left_hand.rotation =
        head_rotation * camera_rotation(&hand_context.left_hand_context);
    input_context.left_hand.thumbstick = left_thumbstick_value;
    input_context.left_hand.trigger_value = f32_from_bool(hand_context.left_trigger_pressed);
    input_context.left_hand.squeeze_value = f32_from_bool(hand_context.left_squeeze_pressed);
    input_context.left_hand.a_value = f32_from_bool(hand_context.left_a_pressed);
    // input_context.left_hand.trigger_value = trigger_value;
    // input_context.left_hand.squeeze_value = squeeze_value;

    // Flatscreen UI (menus) reads an absolute 2D pointer rather than the VR
    // hands. The cursor is in Normal mode here, so positions are window coords.
    if wants_pointer {
        if let Some(MousePosition { x, y }) = &camera_context.mouse_position {
            input_context.pointer = Some(shock2vr::input_context::Pointer2D {
                position: vec2(x / SCR_WIDTH as f32, y / SCR_HEIGHT as f32),
                pressed: window.get_mouse_button(MouseButton::Button1) == Action::Press,
            });
        }
    }

    let mut input_state = InputState::new();
    input_state.is_crouching = window.get_key(Key::LeftControl) == Action::Press;

    // Discrete actions (spawn, save/load, inventory, pathfinding test) are
    // handled by the mapper, which edge-detects key presses.
    input_mapper.poll(window, action_state);

    (input_context, input_state)
}
