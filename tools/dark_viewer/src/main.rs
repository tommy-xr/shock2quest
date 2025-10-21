extern crate glfw;
use dark::model::AnimatedModel;
use dark::model::Model;
use glfw::GlfwReceiver;

mod scenes;
use scenes::{BinObjViewerScene, FontViewerScene, ToolScene, VideoPlayerScene};
use shock2vr::zip_asset_path::ZipAssetPath;

use self::glfw::{Action, Context, Key};
use engine::audio::{self, AudioClip, AudioContext, AudioHandle};

use cgmath::point3;
use cgmath::Decomposed;
use cgmath::Deg;
use cgmath::Matrix4;
use cgmath::Rad;
#[cfg(feature = "ffmpeg")]
use engine_ffmpeg::AudioPlayer;

use cgmath::vec4;
use dark::font;
use dark::font::Font;
use dark::importers::BITMAP_ANIMATION_IMPORTER;
use dark::importers::MODELS_IMPORTER;
use dark::motion;
use dark::motion::AnimationClip;
use dark::motion::AnimationPlayer;
use dark::motion::MotionClip;
use dark::motion::MotionDB;
use dark::motion::MotionInfo;
use dark::ss2_bin_ai_loader;
use dark::ss2_bin_header;
use dark::ss2_cal_loader;
use dark::ss2_skeleton;
use dark::ss2_skeleton::Skeleton;
use engine::assets::asset_cache::AssetCache;
use engine::assets::asset_paths::AssetPath;
use engine::importers::FBX_IMPORTER;
use engine::scene::mesh;
use engine::scene::Scene;
use engine::scene::SceneObject;
use engine::scene::TextVertex;
use num::ToPrimitive;
use shock2vr::command::SaveCommand;
use shock2vr::command::SpawnItemCommand;
use shock2vr::command::TransitionLevelCommand;
use shock2vr::GameOptions;
use tracing::trace;

extern crate gl;

use cgmath::prelude::*;
use cgmath::vec2;
use cgmath::{vec3, Quaternion, Vector3};
use shock2vr::command::Command;

use glfw::MouseButton;
use shock2vr::input_context::InputContext;
use shock2vr::time::Time;
use shock2vr::zip_asset_path;
use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::io::Write;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
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
    distance: f32,
    mouse_position: Option<MousePosition>,
}

impl CameraContext {
    pub fn new() -> CameraContext {
        CameraContext {
            pitch: 90.0,
            yaw: 90.0,
            distance: 10.0,
            mouse_position: None,
        }
    }
}

struct MouseUpdateResult {
    delta_x: f32,
    delta_y: f32,
}

const BASE_PATH: &str = "../../Data";

pub fn resource_path(str: &str) -> String {
    format!("{BASE_PATH}/{str}")
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
    if v {
        1.0
    } else {
        0.0
    }
}

fn find_video_file(filename: &str) -> Option<String> {
    // Define common video paths to try (only for video files that don't use asset_cache)
    let paths_to_try = vec![
        filename.to_string(),
        format!("../../Data/cutscenes/{}", filename),
        format!("Data/cutscenes/{}", filename),
        format!("cutscenes/{}", filename),
    ];

    // Try each path
    for path in paths_to_try {
        if std::path::Path::new(&path).exists() {
            println!("Found video at: {}", path);
            return Some(path);
        }
    }

    println!(
        "Could not find video file {} in any of the expected locations",
        filename
    );
    None
}

fn create_scene(
    filename: &str,
    _animation_file: &Option<String>,
    asset_cache: &engine::assets::asset_cache::AssetCache,
    resource_path: fn(&str) -> String
) -> Result<Box<dyn ToolScene>, Box<dyn std::error::Error>> {
    // Determine scene type from file extension
    if filename.to_lowercase().ends_with(".avi") {
        if let Some(video_path) = find_video_file(filename) {
            let scene = VideoPlayerScene::from_file(video_path)?;
            Ok(Box::new(scene))
        } else {
            Err(format!("Could not find video file: {}", filename).into())
        }
    } else if filename.to_lowercase().ends_with(".bin") {
        let scene = BinObjViewerScene::from_model(filename.to_string(), asset_cache)?;
        Ok(Box::new(scene))
    } else if filename.to_lowercase().ends_with(".fon") {
        let scene = FontViewerScene::from_file(filename.to_string(), resource_path)?;
        Ok(Box::new(scene))
    } else {
        Err(format!("Unsupported file type: {}. Supported file types: .avi (video), .bin (3D model), .fon (font)", filename).into())
    }
}

pub fn main() {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args.len() > 4 {
        eprintln!(
            "Usage: {} <filename> [--animation <animation_file>]",
            args[0]
        );
        eprintln!("Supported file types: .avi (video), .bin (3D model), .fon (font)");
        eprintln!("Optional: --animation <file.cal> for .bin files with skeleton animation");
        std::process::exit(1);
    }

    let filename = &args[1];

    // Parse optional animation flag
    let animation_file = if args.len() == 4 && args[2] == "--animation" {
        Some(args[3].clone())
    } else if args.len() == 3 {
        eprintln!("Error: --animation flag requires an animation filename");
        std::process::exit(1);
    } else {
        None
    };

    if let Some(ref anim_file) = animation_file {
        println!(
            "Loading {} with animation {}",
            filename, anim_file
        );
    } else {
        println!("Loading {}", filename);
    }

    // glfw: initialize and configure
    // ------------------------------

    #[cfg(feature = "ffmpeg")]
    engine_ffmpeg::init().unwrap();
    let mut audio_context: AudioContext<(), String> = AudioContext::new();


    // #[cfg(feature = "ffmpeg")]
    // {
    //     let file_name = "../../Data/cutscenes/cs2.avi";
    //     let clip = AudioPlayer::from_filename(file_name).unwrap();
    //     let handle = AudioHandle::new();
    //     audio::test_audio(&mut audio_context, handle, None, Rc::new(clip));
    // }

    // panic!();
    tracing_subscriber::fmt::init();
    let mut glfw = glfw::init(glfw::fail_on_errors).unwrap();
    // TODO: Figure out ANGLE
    // glfw.window_hint(glfw::WindowHint::ClientApi(glfw::OpenGlEs));
    glfw.window_hint(glfw::WindowHint::ContextVersion(4, 1));
    glfw.window_hint(glfw::WindowHint::OpenGlProfile(
        glfw::OpenGlProfileHint::Core,
    ));
    #[cfg(target_os = "macos")]
    glfw.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));

    // println!("RES: {:?}", res);
    // res.unwrap();
    // glfw window creation
    // --------------------
    let (mut window, events) = glfw
        .create_window(
            SCR_WIDTH,
            SCR_HEIGHT,
            "Shock Engine - Viewer",
            glfw::WindowMode::Windowed,
        )
        .expect("Failed to create GLFW window");

    window.make_current();
    window.set_key_polling(true);
    window.set_scroll_polling(true);
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
    let file_system = engine.get_storage().external_filesystem();
    let mut game = shock2vr::Game::init(file_system, GameOptions::default());

    // Create the appropriate scene based on file type
    let mut scene = match create_scene(filename, &animation_file, &game.asset_cache, resource_path) {
        Ok(scene) => scene,
        Err(err) => {
            eprintln!("Error creating scene: {}", err);
            std::process::exit(1);
        }
    };
    // FOR SCREENSHOT
    // let mut camera_context = CameraContext {
    //     camera_offset: cgmath::Vector3::new(1.25, -14.0, -24.0),
    //     pitch: 4.81,
    //     yaw: -213.0,
    //     mouse_position: None,
    // };

    let motiondb_file = File::open(resource_path("motiondb.bin")).unwrap();
    let mut motiondb_reader = BufReader::new(motiondb_file);

    let mut camera_context = CameraContext::new();

    let mut last_time = glfw.get_time() as f32;
    let start_time = last_time;

    let mut frame = 0;
    // render loop
    // -----------
    while !window.should_close() {
        // events
        // -----
        let time = glfw.get_time() as f32;
        let delta_time = time - last_time;
        last_time = time;

        let (input_context, commands) =
            process_events(&mut window, &mut camera_context, &events, delta_time);
        let ratio = SCR_WIDTH as f32 / SCR_HEIGHT as f32;
        let projection_matrix: cgmath::Matrix4<f32> =
            cgmath::perspective(cgmath::Deg(45.0), ratio, 0.1, 1000.0);

        let time = Time {
            elapsed: Duration::from_secs_f32(delta_time),
            total: Duration::from_secs_f32(time - start_time),
        };

        //let (mut scene, pawn_offset, pawn_rotation) = game.render();

        let mut scene_objects = vec![];

        // Update and render the scene
        scene.update(delta_time);
        let rendered_scene = scene.render(&mut game.asset_cache);
        for obj in rendered_scene.objects {
            scene_objects.push(obj);
        }

        let yaw_rad = camera_context.yaw.to_radians();
        let pitch_rad = camera_context.pitch.to_radians();
        let radius = camera_context.distance;
        let x = radius * pitch_rad.sin() * yaw_rad.cos();
        let y = radius * pitch_rad.cos();
        let z = radius * pitch_rad.sin() * yaw_rad.sin();
        let orig_camera_position = Vector3::new(x, y, z);

        let pitch_quat = Quaternion::from_angle_x(Rad(pitch_rad - 90.0f32.to_radians()));
        let yaw_quat = Quaternion::from_angle_y(Rad(-yaw_rad + 90.0f32.to_radians()));
        let orig_camera_rot = yaw_quat * pitch_quat;

        let mat: Decomposed<Vector3<f32>, Quaternion<f32>> = cgmath::Transform::look_at_rh(
            point3(0.0, 0.0, -1.0),
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
        );

        let cm = engine::scene::color_material::create(vec3(0.0, 1.0, 0.0));

        let orig_camera_forward = orig_camera_rot * vec3(0.0, 0.0, -1.0);
        let pointer_mat = engine::scene::color_material::create(vec3(0.0, 1.0, 0.0));
        let mut pointer_obj =
            SceneObject::new(pointer_mat, Box::new(engine::scene::cube::create()));
        pointer_obj.set_transform(Matrix4::from_translation(
            orig_camera_position + orig_camera_forward,
        ));


        let camera_mat = engine::scene::color_material::create(vec3(1.0, 0.0, 0.0));
        let mut camera_obj = SceneObject::new(camera_mat, Box::new(engine::scene::cube::create()));
        camera_obj.set_transform(Matrix4::from_translation(orig_camera_position));
        camera_obj.set_local_transform(Matrix4::from(orig_camera_rot));

        let unit_vec = vec3(0.0, 0.0, 0.0);
        let unit_quat = Quaternion {
            v: vec3(0.0, 0.0, 0.0),
            s: 1.0,
        };

        let render_context = engine::EngineRenderContext {
            time: glfw.get_time() as f32,
            camera_offset: orig_camera_position,
            camera_rotation: unit_quat,

            head_offset: unit_vec,
            head_rotation: orig_camera_rot,
            // camera_offset: vec3(0.0, 0.0, 0.0),
            // camera_rotation: Quaternion {
            //     v: vec3(0.0, 0.0, 0.0),
            //     s: 1.0,
            // },

            // head_offset: camera_position,
            // head_rotation: rot,
            projection_matrix,

            screen_size: vec2(SCR_WIDTH as f32, SCR_HEIGHT as f32),
        };

        frame += 1;

        let full_scene = Scene::from_objects(scene_objects);
        engine.render(&render_context, &full_scene);

        // glfw: swap buffers and poll IO events (keys pressed/released, mouse moved etc.)
        // -------------------------------------------------------------------------------
        window.swap_buffers();
        glfw.poll_events();
    }
}

// NOTE: not the same version as in common.rs!
fn process_events(
    //audio: &mut AudioContext,
    window: &mut glfw::Window,
    camera_context: &mut CameraContext,
    events: &GlfwReceiver<(f64, glfw::WindowEvent)>,
    delta_time: f32,
) -> (InputContext, Vec<Box<dyn Command>>) {
    let _speed = 20.0;
    let head_rot_speed = 10.0;

    let _movement = cgmath::Vector3::new(0.0, 0.0, 0.0);
    let mut commands: Vec<Box<dyn Command>> = vec![];
    //let mut forward = cgmath::Vector3::new(0.0, );

    trace!("delta time: {delta_time}");
    let mut rot_yaw = 0.0;
    let mut rot_pitch = 0.0;
    let mut delta_zoom = 0.0;

    for (_, event) in glfw::flush_messages(events) {
        match event {
            glfw::WindowEvent::FramebufferSize(width, height) => {
                // make sure the viewport matches the new window dimensions; note that width and
                // height will be significantly larger than specified on retina displays.
                unsafe { gl::Viewport(0, 0, width, height) }
            }
            // glfw::WindowEvent::Key(Key::Space, _, Action::Press, _) => {
            //     engine::audio::test_audio(audio)
            // }
            glfw::WindowEvent::Key(Key::Escape, _, Action::Press, _) => {
                window.set_should_close(true)
            }
            glfw::WindowEvent::CursorPos(x, y) => {
                let mouse_update = camera_update_mouse(camera_context, x as f32, y as f32);
                rot_yaw = 1.0 * mouse_update.delta_x;
                rot_pitch = 1.0 * mouse_update.delta_y;
            }
            glfw::WindowEvent::Scroll(x, y) => {
                delta_zoom = y as f32 * 2.0;
            }
            _ => {}
        }
    }

    camera_context.yaw += rot_yaw;
    camera_context.pitch += rot_pitch;
    camera_context.distance += delta_zoom;

    if camera_context.distance < 1.0 {
        camera_context.distance = 1.0;
    }

    if camera_context.distance > 100.0 {
        camera_context.distance = 100.0;
    }

    let mut input_context = InputContext::default();
    let head_rotation = camera_rotation(camera_context);
    (input_context, commands)
}

// fn load_animation(motiondb: &MotionDB, name: String) -> AnimationClip {
//     let mps_motion = motiondb.get_mps_motions(name.to_owned());

//     let motion_info_path = format!("res/motions/{name}.mi");
//     let motion_info_file = File::open(resource_path(&motion_info_path)).unwrap();
//     let mut motion_info_reader = BufReader::new(motion_info_file);
//     let motion_info = MotionInfo::read(&mut motion_info_reader);

//     // panic!("frame rate: {}", motion_info.frame_rate);

//     let motion_file = File::open(resource_path(&format!("res/motions/{name}_.mc"))).unwrap();
//     let mut motion_reader = BufReader::new(motion_file);
//     let motion = MotionClip::read(&mut motion_reader, &motion_info, mps_motion);

//     AnimationClip::create(&motion, &motion_info, mps_motion)
// }
