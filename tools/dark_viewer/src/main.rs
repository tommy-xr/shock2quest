#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

extern crate glfw;
use clap::Parser;
use glfw::GlfwReceiver;

mod scenes;
use scenes::{
    BinAiViewerScene, BinObjViewerScene, FontViewerScene, GlbAnimatedViewerScene, GlbViewerScene,
    ToolScene, VideoPlayerScene,
};
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
use engine::assets::asset_cache::AssetCache;
use engine::assets::asset_paths::AssetPath;
use engine::scene::Scene;
use engine::scene::SceneObject;
use engine::scene::TextVertex;
use shock2vr::command::Command;
use shock2vr::command::SaveCommand;
use shock2vr::command::SpawnItemCommand;
use shock2vr::command::TransitionLevelCommand;
use shock2vr::paths;
use shock2vr::GameOptions;
use tracing::trace;

extern crate gl;

use cgmath::prelude::*;
use cgmath::vec2;
use cgmath::{vec3, Quaternion, Vector3};
use shock2vr::input_context::InputContext;
use shock2vr::time::Time;
use shock2vr::zip_asset_path;
use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

const SCR_WIDTH: u32 = 800;
const SCR_HEIGHT: u32 = 600;

#[derive(Parser, Debug)]
#[command(author, version, about = "Shock Engine tooling viewer", long_about = None)]
struct Cli {
    /// Asset to preview (video .avi, object .bin, font .fon, GLB .glb)
    filename: String,

    /// One or more animation clips, comma separated.
    /// For .bin files: Paths can be provided with or without the trailing `_ .mc` suffix.
    /// For .glb files: Animation names from within the GLB file. Use flag without value to show all.
    #[arg(long, value_delimiter = ',', value_name = "CLIP", num_args = 0..)]
    animation: Option<Vec<String>>,

    /// When true, loads assets and prints information but exits before opening a window.
    #[arg(long)]
    debug_no_render: bool,
}

fn resolve_data_path(resource: &str) -> String {
    paths::data_root()
        .join(resource)
        .to_string_lossy()
        .into_owned()
}

fn normalize_clip_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Animation name may not be empty".to_owned());
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("_.mc") {
        return Ok(trimmed.to_owned());
    }
    if lower.ends_with(".mc") {
        let without_ext = &trimmed[..trimmed.len() - 3];
        return Ok(format!("{}_.mc", without_ext));
    }
    Ok(format!("{}_.mc", trimmed))
}

fn gather_animation_list(cli: &Cli, filename: &str) -> Result<(Vec<String>, bool), String> {
    let animation_flag_provided = cli.animation.is_some();

    let animation_list = match &cli.animation {
        None => return Ok((Vec::new(), false)),
        Some(animations) => animations,
    };

    let is_glb = filename.to_ascii_lowercase().ends_with(".glb");

    if is_glb {
        // For GLB files, use animation names as-is (no .mc suffix normalization)
        // Filter out empty strings - empty means "show all animations"
        let filtered_animations = animation_list
            .iter()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .collect();
        Ok((filtered_animations, animation_flag_provided))
    } else {
        // For BIN files, apply the .mc suffix normalization
        let animations = animation_list
            .iter()
            .map(|raw| normalize_clip_name(raw))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((animations, animation_flag_provided))
    }
}

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

fn find_video_file(filename: &str) -> Option<String> {
    let requested = Path::new(filename);
    let data_root = paths::data_root();

    let mut candidates = vec![requested.to_path_buf()];
    if requested.is_relative() {
        candidates.push(data_root.join(requested));
        if let Some(file_name) = requested.file_name() {
            candidates.push(data_root.join("cutscenes").join(file_name));
            candidates.push(Path::new("cutscenes").join(file_name));
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            println!("Found video at: {}", candidate.display());
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    println!("Could not find video file {} in any of the expected locations (searched under {} and current directory)", filename, data_root.display());
    None
}

fn create_scene(
    filename: &str,
    animations: &[String],
    animation_flag_provided: bool,
    asset_cache: &mut engine::assets::asset_cache::AssetCache,
    data_resolver: fn(&str) -> String,
) -> Result<Box<dyn ToolScene>, Box<dyn std::error::Error>> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".avi") {
        if let Some(video_path) = find_video_file(filename) {
            let scene = VideoPlayerScene::from_file(video_path)?;
            Ok(Box::new(scene))
        } else {
            Err(format!("Could not find video file: {}", filename).into())
        }
    } else if lower.ends_with(".bin") {
        if animations.is_empty() {
            let scene = BinObjViewerScene::from_model(filename.to_string(), asset_cache)?;
            Ok(Box::new(scene))
        } else {
            let scene = BinAiViewerScene::from_clips(
                filename.to_string(),
                animations.to_vec(),
                asset_cache,
            )?;
            Ok(Box::new(scene))
        }
    } else if lower.ends_with(".fon") {
        let scene = FontViewerScene::from_file(filename.to_string(), data_resolver)?;
        Ok(Box::new(scene))
    } else if lower.ends_with(".glb") {
        if animation_flag_provided {
            let scene = GlbAnimatedViewerScene::from_model_and_animations(
                filename.to_string(),
                animations.to_vec(),
                asset_cache,
            )?;
            Ok(Box::new(scene))
        } else {
            let scene = GlbViewerScene::from_model(filename.to_string(), asset_cache)?;
            Ok(Box::new(scene))
        }
    } else if !animations.is_empty() {
        Err("Animation preview is only supported for .bin AI meshes and .glb models.".into())
    } else {
        Err(format!(
            "Unsupported file type: {}. Supported file types: .avi (video), .bin (3D model), .fon (font), .glb (GLB/GLTF 3D model)",
            filename
        )
        .into())
    }
}

pub fn main() {
    let cli = Cli::parse();
    let (animations, animation_flag_provided) = match gather_animation_list(&cli, &cli.filename) {
        Ok((list, flag_provided)) => (list, flag_provided),
        Err(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    };

    let filename = cli.filename.clone();

    if animations.is_empty() {
        println!("Loading {}", filename);
    } else {
        let summary = animations.join(", ");
        println!("Loading {filename} with animations: {summary}");
    }

    if cli.debug_no_render {
        println!("Debug no-render mode enabled.");
    }

    let mut audio_context: AudioContext<(), String> = AudioContext::new();

    tracing_subscriber::fmt::init();
    let mut glfw = glfw::init(glfw::fail_on_errors).unwrap();
    glfw.window_hint(glfw::WindowHint::ContextVersion(4, 1));
    glfw.window_hint(glfw::WindowHint::OpenGlProfile(
        glfw::OpenGlProfileHint::Core,
    ));
    #[cfg(target_os = "macos")]
    glfw.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));

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
    let asset_path = shock2vr::paths::asset_root().to_string_lossy().into_owned();
    let mut game = shock2vr::Game::init(GameOptions::default(), asset_path);

    if cli.debug_no_render {
        match create_scene(
            &filename,
            &animations,
            animation_flag_provided,
            &mut game.asset_cache,
            resolve_data_path,
        ) {
            Ok(_) => println!("Scene creation succeeded."),
            Err(err) => println!("Error creating scene: {err}"),
        }
        return;
    }

    let mut scene = match create_scene(
        &filename,
        &animations,
        animation_flag_provided,
        &mut game.asset_cache,
        resolve_data_path,
    ) {
        Ok(scene) => scene,
        Err(err) => {
            eprintln!("Error creating scene: {}", err);
            std::process::exit(1);
        }
    };

    // Initialize the scene with audio context
    scene.init(&mut audio_context);

    let mut camera_context = CameraContext::new();

    let mut last_time = glfw.get_time() as f32;
    let start_time = last_time;

    while !window.should_close() {
        let time = glfw.get_time() as f32;
        let delta_time = time - last_time;
        last_time = time;

        let (_input_context, _commands) =
            process_events(&mut window, &mut camera_context, &events, delta_time);
        let ratio = SCR_WIDTH as f32 / SCR_HEIGHT as f32;
        let projection_matrix: cgmath::Matrix4<f32> =
            cgmath::perspective(cgmath::Deg(45.0), ratio, 0.1, 1000.0);

        scene.update(delta_time);
        let rendered_scene = scene.render(&mut game.asset_cache);
        let scene_objects = rendered_scene.objects;

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

        let render_context = engine::EngineRenderContext {
            time: glfw.get_time() as f32,
            camera_offset: orig_camera_position,
            camera_rotation: Quaternion {
                v: vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            head_offset: vec3(0.0, 0.0, 0.0),
            head_rotation: orig_camera_rot,
            projection_matrix,
            screen_size: vec2(SCR_WIDTH as f32, SCR_HEIGHT as f32),
        };

        let full_scene = Scene::from_objects(scene_objects);
        engine.render(&render_context, &full_scene);

        window.swap_buffers();
        glfw.poll_events();
    }
}

fn process_events(
    window: &mut glfw::Window,
    camera_context: &mut CameraContext,
    events: &GlfwReceiver<(f64, glfw::WindowEvent)>,
    _delta_time: f32,
) -> (InputContext, Vec<Box<dyn Command>>) {
    for (_, event) in glfw::flush_messages(events) {
        match event {
            glfw::WindowEvent::FramebufferSize(width, height) => unsafe {
                gl::Viewport(0, 0, width, height)
            },
            glfw::WindowEvent::Key(Key::Escape, _, Action::Press, _) => {
                window.set_should_close(true)
            }
            glfw::WindowEvent::CursorPos(x, y) => {
                let mouse_update = camera_update_mouse(camera_context, x as f32, y as f32);
                camera_context.yaw += mouse_update.delta_x;
                camera_context.pitch += mouse_update.delta_y;
            }
            glfw::WindowEvent::Scroll(_, y) => {
                camera_context.distance = (camera_context.distance - y as f32).clamp(1.0, 100.0);
            }
            _ => {}
        }
    }

    (InputContext::default(), Vec::new())
}
