// Debug Runtime - HTTP-controlled game runtime for LLM testing and automation
//
// This runtime provides a localhost-only HTTP API for controlling the game,
// enabling LLMs and automation scripts to test gameplay, debug issues, and
// validate changes without requiring human interaction.

use axum::{extract::State, response::Json, routing::get, Router};
use clap::Parser;
use serde_json::{json, Value};
use std::{collections::HashSet, net::SocketAddr, time::Duration};
use tokio::{signal, sync::mpsc, sync::oneshot};
use tracing::info;

mod commands;
use commands::*;

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

// Property imports for state queries
use dark::properties::{PropModelName, PropPosition, PropSymName, PropTemplateId};
use shipyard::{Get, IntoIter, IntoWithId, View};

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

    // Create command channel for communication between HTTP server and game loop
    let (command_tx, command_rx) = mpsc::unbounded_channel::<RuntimeCommand>();

    // Start the HTTP server in a background task
    let server_handle = rt.spawn(start_http_server(args.port, command_tx));

    // Run the game on the main thread (required for GLFW)
    let game_result = run_game_blocking(args, command_rx);

    // If game exits, shutdown the server
    server_handle.abort();

    game_result?;

    Ok(())
}

/// Start the HTTP server
async fn start_http_server(
    port: u16,
    command_tx: mpsc::UnboundedSender<RuntimeCommand>,
) -> anyhow::Result<()> {
    // Create the router with health endpoint
    let app = Router::new()
        .route("/v1/health", get(health_check))
        .route("/v1/info", get(get_info))
        .route("/v1/step", axum::routing::post(step_frame))
        .route("/v1/shutdown", axum::routing::post(shutdown_server))
        .with_state(command_tx);

    // Bind to localhost only for security
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("Debug runtime listening on http://{}", addr);

    // Log available endpoints
    info!("Available endpoints:");
    info!("  GET  /v1/health           - Health check and server status");
    info!("  GET  /v1/info             - Get current game state snapshot");
    info!("  POST /v1/step             - Step the simulation forward");
    info!("  POST /v1/shutdown         - Shutdown the debug runtime gracefully");
    info!("  (More endpoints coming in Phase 2)");
    info!("");
    info!("Test with: curl http://{}/v1/health", addr);
    info!("Test with: curl http://{}/v1/info", addr);
    info!("Test with: curl -X POST http://{}/v1/step", addr);
    info!("Test with: curl -X POST http://{}/v1/shutdown", addr);

    // Start the server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Run the game loop (blocking)
fn run_game_blocking(
    args: Args,
    mut command_rx: mpsc::UnboundedReceiver<RuntimeCommand>,
) -> anyhow::Result<()> {
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

    // Debug runtime execution control
    let mut is_paused = true; // Start paused by default
    let mut step_requested = false;
    let mut accumulated_time = 0.0f32;
    let mut shutdown_requested = false;
    let mut frame_counter = 0u64;
    let mut frames_to_step = 0u32;
    let mut target_step_time: Option<f32> = None;

    info!("Starting main game loop...");
    info!("Game is PAUSED by default - use /v1/step to advance frames");

    // Main game loop
    while !window.should_close() && !shutdown_requested {
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

        let game_time = Time {
            elapsed: Duration::from_secs_f32(delta_time),
            total: Duration::from_secs_f32(time - start_time),
        };

        // Process commands from HTTP server
        while let Ok(command) = command_rx.try_recv() {
            match &command {
                RuntimeCommand::Step(step_spec, _) => {
                    match step_spec {
                        StepSpec::Frames { frames } => {
                            frames_to_step = *frames;
                            target_step_time = None;
                            step_requested = true;
                            is_paused = false;
                            tracing::info!("Starting step: {} frames", frames);
                        }
                        StepSpec::Duration { duration } => {
                            // Parse duration string using humantime
                            match duration.parse::<humantime::Duration>() {
                                Ok(parsed_duration) => {
                                    let duration_secs = parsed_duration.as_secs_f32();
                                    target_step_time = Some(accumulated_time + duration_secs);
                                    frames_to_step = 0;
                                    step_requested = true;
                                    is_paused = false;
                                    tracing::info!("Starting step: {} ({:.3}s)", duration, duration_secs);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse duration '{}': {}", duration, e);
                                }
                            }
                        }
                    }
                }
                RuntimeCommand::Shutdown => {
                    shutdown_requested = true;
                    tracing::info!("Shutdown requested via API");
                }
                _ => {}
            }
            process_command(command, &game, &game_time, frame_counter);
        }

        // No commands for now
        let commands: Vec<Box<dyn Command>> = vec![];

        // Only update the game if not paused or if step was requested
        let actual_game_time = if !is_paused || step_requested {
            profile!(
                "game.update",
                game.update(&game_time, &input_context, commands)
            );

            if step_requested {
                // Increment frame counter and accumulated time
                frame_counter += 1;
                accumulated_time += game_time.elapsed.as_secs_f32();

                // Check if we should continue stepping or pause
                let should_continue = if let Some(target_time) = target_step_time {
                    // Time-based stepping
                    if accumulated_time >= target_time {
                        tracing::info!(
                            "Time-based step completed: reached {:.3}s after {} frames",
                            accumulated_time, frame_counter
                        );
                        false
                    } else {
                        true
                    }
                } else if frames_to_step > 0 {
                    // Frame-based stepping
                    frames_to_step -= 1;
                    if frames_to_step == 0 {
                        tracing::info!(
                            "Frame-based step completed: {} frames, total time: {:.3}s",
                            frame_counter, accumulated_time
                        );
                        false
                    } else {
                        true
                    }
                } else {
                    // Single frame step (legacy behavior)
                    tracing::info!(
                        "Stepped 1 frame, game paused again. Frame: {}, Total time: {:.3}s",
                        frame_counter, accumulated_time
                    );
                    false
                };

                if !should_continue {
                    step_requested = false;
                    is_paused = true;
                    target_step_time = None;
                    frames_to_step = 0;
                }
            }
            accumulated_time
        } else {
            // When paused, use zero delta time to prevent any updates
            let zero_time = Time {
                elapsed: Duration::from_secs_f32(0.0),
                total: Duration::from_secs_f32(accumulated_time),
            };
            // Still call update with zero time to maintain state consistency
            profile!(
                "game.update",
                game.update(&zero_time, &input_context, commands)
            );
            accumulated_time // Use accumulated time, not real time
        };

        // Render the game
        let ratio = SCR_WIDTH as f32 / SCR_HEIGHT as f32;
        let projection_matrix: cgmath::Matrix4<f32> =
            cgmath::perspective(cgmath::Deg(45.0), ratio, 0.1, 1000.0);

        let screen_size = vec2(SCR_WIDTH as f32, SCR_HEIGHT as f32);

        let (mut scene, pawn_offset, pawn_rotation) = profile!("game.render", game.render());

        // Create a simple render context for debug view
        let render_context = EngineRenderContext {
            time: actual_game_time, // Use accumulated game time, not real time
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

    if shutdown_requested {
        info!("Game loop ended due to shutdown request");
    } else {
        info!("Game loop ended due to window close");
    }
    Ok(())
}

/// Process a command from the HTTP server
fn process_command(command: RuntimeCommand, game: &Game, time: &Time, frame_counter: u64) {
    match command {
        RuntimeCommand::GetInfo(reply) => {
            let snapshot = capture_frame_snapshot(game, time, frame_counter);
            if let Err(_) = reply.send(snapshot) {
                tracing::warn!("Failed to send frame snapshot - receiver dropped");
            }
        }
        RuntimeCommand::Step(spec, reply) => {
            // Step command will be handled by the game loop logic above
            // Return result based on the step specification
            let (frames_requested, time_requested) = match spec {
                StepSpec::Frames { frames } => (frames, None),
                StepSpec::Duration { duration } => {
                    let parsed_time = duration.parse::<humantime::Duration>()
                        .map(|d| d.as_secs_f32())
                        .unwrap_or(0.0);
                    (0, Some(parsed_time))
                }
            };

            let result = StepResult {
                frames_advanced: frames_requested.max(1), // At least 1 frame will be processed
                time_advanced: time_requested.unwrap_or(0.016), // Default ~60fps frame time
                new_frame_index: frame_counter,
                new_total_time: time.total.as_secs_f32(),
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send step result - receiver dropped");
            }
        }
        RuntimeCommand::Screenshot(spec, reply) => {
            // TODO: Implement screenshot logic
            let result = ScreenshotResult {
                filename: spec
                    .filename
                    .unwrap_or_else(|| "screenshot.png".to_string()),
                full_path: "/tmp/screenshot.png".to_string(),
                resolution: [SCR_WIDTH, SCR_HEIGHT],
                size_bytes: 0,
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send screenshot result - receiver dropped");
            }
        }
        RuntimeCommand::RayCast(request, reply) => {
            // TODO: Implement raycast logic
            let result = RayCastResult {
                hit: false,
                hit_point: None,
                hit_normal: None,
                distance: None,
                entity_id: None,
                entity_name: None,
                collision_group: None,
                is_sensor: false,
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send raycast result - receiver dropped");
            }
        }
        RuntimeCommand::SetInput(_patch) => {
            // TODO: Implement input modification
            tracing::info!("Input modification not yet implemented");
        }
        RuntimeCommand::MovePlayer(_position) => {
            // TODO: Implement player movement
            tracing::info!("Player movement not yet implemented");
        }
        RuntimeCommand::RunGameCommand(_command, _args, reply) => {
            // TODO: Implement game command execution
            let result = CommandResult {
                success: false,
                message: "Game commands not yet implemented".to_string(),
                data: None,
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send command result - receiver dropped");
            }
        }
        RuntimeCommand::ListEntities { limit: _, reply } => {
            // TODO: Implement entity listing
            let result = EntityListResult {
                entities: vec![],
                total_count: 0,
                player_position: [0.0, 0.0, 0.0],
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send entity list - receiver dropped");
            }
        }
        RuntimeCommand::EntityDetail { id: _, reply } => {
            // TODO: Implement entity detail
            let result = EntityDetailResult {
                entity_id: 0,
                name: "Unknown".to_string(),
                template_id: 0,
                position: [0.0, 0.0, 0.0],
                rotation: [1.0, 0.0, 0.0, 0.0],
                inheritance_chain: vec![],
                properties: vec![],
                outgoing_links: vec![],
                incoming_links: vec![],
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send entity detail - receiver dropped");
            }
        }
        RuntimeCommand::Shutdown => {
            // Shutdown is handled in the main loop, this is just for completeness
            tracing::info!("Processing shutdown command");
        }
    }
}

/// Capture current game state as a frame snapshot
fn capture_frame_snapshot(game: &Game, time: &Time, frame_counter: u64) -> FrameSnapshot {
    let world = game.world();

    // Query entity count by getting all entities with template IDs
    let entity_count =
        world.run(|v_template_id: View<PropTemplateId>| v_template_id.iter().with_id().count());

    // Log a sample of entities for debugging
    let _sample_entities: Vec<String> = world.run(
        |v_template_id: View<PropTemplateId>,
         v_position: View<PropPosition>,
         v_symname: View<PropSymName>,
         v_model: View<PropModelName>| {
            v_template_id
                .iter()
                .with_id()
                .take(10) // Limit to first 10 entities
                .map(|(entity_id, template_id)| {
                    let pos_str = if let Ok(pos) = v_position.get(entity_id) {
                        format!(
                            "pos:[{:.2},{:.2},{:.2}]",
                            pos.position.x, pos.position.y, pos.position.z
                        )
                    } else {
                        "pos:none".to_string()
                    };

                    let name_str = if let Ok(symname) = v_symname.get(entity_id) {
                        format!("name:{}", symname.0)
                    } else {
                        "name:none".to_string()
                    };

                    let model_str = if let Ok(model) = v_model.get(entity_id) {
                        format!("model:{}", model.0)
                    } else {
                        "model:none".to_string()
                    };

                    let entity_info = format!(
                        "entity_id:{} template_id:{} {} {} {}",
                        entity_id.inner(),
                        template_id.template_id,
                        name_str,
                        pos_str,
                        model_str
                    );

                    tracing::info!("Entity: {}", entity_info);
                    entity_info
                })
                .collect()
        },
    );

    // TODO: Find player entity specifically
    // TODO: Get actual mission name from game scene
    // TODO: Track frame counter

    FrameSnapshot {
        frame_index: frame_counter,
        time: TimeInfo {
            elapsed_ms: time.elapsed.as_millis() as f32,
            total_ms: time.total.as_millis() as f32,
        },
        mission: "earth.mis".to_string(), // TODO: Get actual mission name
        player: PlayerInfo {
            entity_id: None,                       // TODO: Get player entity ID
            position: [0.0, 0.0, 0.0],             // TODO: Get player position
            rotation: [1.0, 0.0, 0.0, 0.0],        // TODO: Get player rotation
            camera_offset: [0.0, 1.6, 0.0],        // TODO: Get camera offset
            camera_rotation: [1.0, 0.0, 0.0, 0.0], // TODO: Get camera rotation
        },
        entity_count,
        debug_features: vec![], // TODO: List active debug features
        inputs: InputSnapshot {
            head_rotation: [1.0, 0.0, 0.0, 0.0],
            hands: HandsSnapshot {
                left: HandSnapshot {
                    position: [0.0, 0.0, 0.0],
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    thumbstick: [0.0, 0.0],
                    trigger: 0.0,
                    squeeze: 0.0,
                    a: 0.0,
                },
                right: HandSnapshot {
                    position: [0.0, 0.0, 0.0],
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    thumbstick: [0.0, 0.0],
                    trigger: 0.0,
                    squeeze: 0.0,
                    a: 0.0,
                },
            },
        },
    }
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

/// Get current game state snapshot
async fn get_info(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Json<FrameSnapshot> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::GetInfo(reply_tx)) {
        tracing::error!("Failed to send GetInfo command - game loop receiver dropped");
        return Json(FrameSnapshot::new());
    }

    // Wait for response
    match reply_rx.await {
        Ok(snapshot) => Json(snapshot),
        Err(_) => {
            tracing::error!("Failed to receive frame snapshot - sender dropped");
            Json(FrameSnapshot::new())
        }
    }
}

/// Step the simulation forward by one frame or time duration
async fn step_frame(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Json(step_spec): Json<StepSpec>,
) -> Json<StepResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::Step(step_spec, reply_tx)) {
        tracing::error!("Failed to send Step command - game loop receiver dropped");
        return Json(StepResult {
            frames_advanced: 0,
            time_advanced: 0.0,
            new_frame_index: 0,
            new_total_time: 0.0,
        });
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive step result - sender dropped");
            Json(StepResult {
                frames_advanced: 0,
                time_advanced: 0.0,
                new_frame_index: 0,
                new_total_time: 0.0,
            })
        }
    }
}

/// Shutdown the debug runtime gracefully
async fn shutdown_server(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Json<Value> {
    tracing::info!("Shutdown request received via HTTP API");

    // Send shutdown command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::Shutdown) {
        tracing::error!("Failed to send Shutdown command - game loop receiver dropped");
        return Json(json!({
            "status": "error",
            "message": "Failed to send shutdown command to game loop",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));
    }

    Json(json!({
        "status": "shutting_down",
        "message": "Debug runtime shutdown initiated",
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
