// Debug Runtime - HTTP-controlled game runtime for LLM testing and automation
//
// This runtime provides a localhost-only HTTP API for controlling the game,
// enabling LLMs and automation scripts to test gameplay, debug issues, and
// validate changes without requiring human interaction.

use axum::{response::Json, routing::get, Router};
use clap::Parser;
use serde_json::{json, Value};
use std::{collections::HashSet, net::SocketAddr, time::Duration};
use tokio::signal;
use tracing::info;

// Game engine imports
extern crate glfw;
use self::glfw::{Context, WindowEvent};
use cgmath::{vec2, vec3, Quaternion};
use dark::SCALE_FACTOR;
use engine::{
    profile, scene::Scene, util::compute_view_matrix_from_render_context, EngineRenderContext,
};
use shock2vr::{
    command::Command, input_context::InputContext, time::Time, Game, GameOptions, SpawnLocation,
};

// Screen dimensions for the debug window
const SCR_WIDTH: u32 = 800;
const SCR_HEIGHT: u32 = 600;

#[derive(Parser)]
#[command(name = "debug_runtime")]
#[command(about = "HTTP-controlled game runtime for LLM testing and automation")]
struct Args {
    /// Mission file to load (e.g., medsci1.mis)
    #[arg(short, long, default_value = "earth.mis")]
    mission: String,

    /// Port to bind HTTP server to
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Enable debug physics rendering
    #[arg(long)]
    debug_physics: bool,

    /// Enable debug portals rendering
    #[arg(long)]
    debug_portals: bool,

    /// Enable debug drawing
    #[arg(long)]
    debug_draw: bool,

    /// Show entity IDs
    #[arg(long)]
    debug_show_ids: bool,

    /// Save file to load
    #[arg(short, long)]
    save_file: Option<String>,

    /// Enable experimental features (comma-separated)
    #[arg(long)]
    experimental: Option<String>,
}

/// Parse mission string (supports mission:spawn_location format)
fn parse_mission(mission: &str) -> (String, SpawnLocation) {
    if !mission.contains(':') {
        return (mission.to_owned(), SpawnLocation::MapDefault);
    }
    let parts: Vec<&str> = mission.split(':').collect();
    if parts.len() > 2 {
        panic!("Unable to parse mission argument: {}", mission);
    }
    let mission = parts[0];
    let spawn_location = if parts[1].contains(',') {
        let vec_parts: Vec<&str> = parts[1].split(',').collect();
        if vec_parts.len() != 3 {
            panic!("Unable to parse spawn location: {}", parts[1]);
        }
        let x = vec_parts[0].parse::<f32>().unwrap();
        let y = vec_parts[1].parse::<f32>().unwrap();
        let z = vec_parts[2].parse::<f32>().unwrap();
        SpawnLocation::PositionRotation(vec3(x, y, z), Quaternion::new(1.0, 0.0, 0.0, 0.0))
    } else {
        SpawnLocation::MapDefault
    };
    (mission.to_owned(), spawn_location)
}

fn main() -> anyhow::Result<()> {
    // Initialize tracing with info level by default
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug_runtime=info".into()),
        )
        .init();

    let args = Args::parse();

    info!(
        "Starting debug runtime on port {} with mission: {}",
        args.port, args.mission
    );

    // Create a tokio runtime for the HTTP server
    let rt = tokio::runtime::Runtime::new()?;

    // Start the HTTP server in a background task
    let server_handle = rt.spawn(start_http_server(args.port));

    // Run the game on the main thread (required for GLFW)
    let game_result = run_game_blocking(args);

    // If game exits, shutdown the server
    server_handle.abort();

    game_result?;

    Ok(())
}

/// Start the HTTP server
async fn start_http_server(port: u16) -> anyhow::Result<()> {
    // Create the router with health endpoint
    let app = Router::new().route("/v1/health", get(health_check));

    // Bind to localhost only for security
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("Debug runtime listening on http://{}", addr);

    // Log available endpoints
    info!("Available endpoints:");
    info!("  GET  /v1/health           - Health check and server status");
    info!("  (More endpoints coming in Phase 2)");
    info!("");
    info!("Test with: curl http://{}/v1/health", addr);

    // Start the server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Run the game loop (blocking)
fn run_game_blocking(args: Args) -> anyhow::Result<()> {
    info!("Initializing game engine...");

    // Initialize GLFW
    info!("Step 1: Initializing GLFW...");
    let mut glfw = glfw::init(glfw::fail_on_errors)?;
    info!("GLFW initialized successfully");
    glfw.window_hint(glfw::WindowHint::ContextVersion(4, 1));
    glfw.window_hint(glfw::WindowHint::OpenGlProfile(
        glfw::OpenGlProfileHint::Core,
    ));
    #[cfg(target_os = "macos")]
    glfw.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));

    // Create window
    info!("Step 2: Creating GLFW window...");
    let (mut window, events) = glfw
        .create_window(
            SCR_WIDTH,
            SCR_HEIGHT,
            "Debug Runtime - Game View",
            glfw::WindowMode::Windowed,
        )
        .expect("Failed to create GLFW window");
    info!("GLFW window created successfully");

    info!("Step 3: Setting up OpenGL context...");
    window.make_current();
    window.set_key_polling(true);
    window.set_framebuffer_size_polling(true);

    // Load OpenGL function pointers
    gl::load_with(|symbol| window.get_proc_address(symbol) as *const _);

    info!("OpenGL initialized successfully");

    // Initialize the game engine
    info!("Step 4: Initializing engine...");
    let engine = engine::opengl();
    info!("Engine initialized successfully");

    info!("Step 5: Setting up game options...");
    let experimental_features: HashSet<String> = args
        .experimental
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let (mission, spawn_location) = parse_mission(&args.mission);
    info!("Mission parsed: {} with spawn location", mission);

    let options = GameOptions {
        mission: mission.clone(),
        spawn_location,
        save_file: args.save_file,
        debug_draw: args.debug_draw,
        debug_physics: args.debug_physics,
        debug_portals: args.debug_portals,
        debug_show_ids: args.debug_show_ids,
        render_particles: true,
        experimental_features,
        ..GameOptions::default()
    };

    info!("Step 6: Initializing game with mission: {}", mission);
    let asset_path = shock2vr::paths::asset_root().to_string_lossy().into_owned();
    info!("Asset path: {}", asset_path);

    let mut game = Game::init(options, asset_path);

    info!("Game initialized successfully with mission: {}", mission);

    let mut last_time = glfw.get_time() as f32;
    let start_time = last_time;

    info!("Starting main game loop...");

    // Main game loop
    while !window.should_close() {
        // Calculate delta time
        let time = glfw.get_time() as f32;
        let delta_time = time - last_time;
        last_time = time;

        // Process GLFW events
        glfw.poll_events();
        for (_, event) in glfw::flush_messages(&events) {
            match event {
                WindowEvent::Key(glfw::Key::Escape, _, glfw::Action::Press, _) => {
                    window.set_should_close(true);
                }
                WindowEvent::FramebufferSize(width, height) => unsafe {
                    gl::Viewport(0, 0, width, height);
                },
                _ => {}
            }
        }

        // Create minimal input context (no actual input for now)
        let input_context = InputContext {
            head: shock2vr::input_context::Head {
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            left_hand: shock2vr::input_context::Hand {
                position: vec3(0.0, 0.0, 0.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                thumbstick: vec2(0.0, 0.0),
                trigger_value: 0.0,
                squeeze_value: 0.0,
                a_value: 0.0,
            },
            right_hand: shock2vr::input_context::Hand {
                position: vec3(0.0, 0.0, 0.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                thumbstick: vec2(0.0, 0.0),
                trigger_value: 0.0,
                squeeze_value: 0.0,
                a_value: 0.0,
            },
        };

        let time = Time {
            elapsed: Duration::from_secs_f32(delta_time),
            total: Duration::from_secs_f32(time - start_time),
        };

        // No commands for now
        let commands: Vec<Box<dyn Command>> = vec![];

        // Update the game
        profile!("game.update", game.update(&time, &input_context, commands));

        // Render the game
        let ratio = SCR_WIDTH as f32 / SCR_HEIGHT as f32;
        let projection_matrix: cgmath::Matrix4<f32> =
            cgmath::perspective(cgmath::Deg(45.0), ratio, 0.1, 1000.0);

        let screen_size = vec2(SCR_WIDTH as f32, SCR_HEIGHT as f32);

        let (mut scene, pawn_offset, pawn_rotation) = profile!("game.render", game.render());

        // Create a simple render context for debug view
        let render_context = EngineRenderContext {
            time: glfw.get_time() as f32,
            camera_offset: pawn_offset,
            camera_rotation: pawn_rotation,
            head_offset: vec3(0.0, 1.6 / SCALE_FACTOR, 0.0), // Default head height
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity rotation
            projection_matrix,
            screen_size,
        };

        let view = compute_view_matrix_from_render_context(&render_context);

        // Render per eye to get the scene objects
        let per_eye_scene = profile!(
            "game.render_per_eye",
            game.render_per_eye(view, projection_matrix, screen_size)
        );

        // Clear the screen to a visible color first
        unsafe {
            gl::ClearColor(0.1, 0.2, 0.3, 1.0); // Dark blue background
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }

        // Combine scene objects
        scene.extend(per_eye_scene);

        // Create the final scene for rendering
        let mut scene_for_render = Scene::from_objects(scene);

        // Add hand spotlights
        let hand_spotlights = game.get_hand_spotlights();
        for spotlight in hand_spotlights {
            scene_for_render.lights_mut().add_spotlight(spotlight);
        }

        // Actually render the scene
        profile!(
            "engine.render",
            engine.render(&render_context, &scene_for_render)
        );

        profile!("game.finish_render", {
            game.finish_render(view, projection_matrix, screen_size)
        });

        // Swap buffers
        window.swap_buffers();
    }

    info!("Game loop ended");
    Ok(())
}

/// Health check endpoint
async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "debug_runtime",
        "version": "0.1.0",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Wait for shutdown signal (Ctrl+C)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down gracefully...");
        },
        _ = terminate => {
            info!("Received SIGTERM, shutting down gracefully...");
        },
    }
}
