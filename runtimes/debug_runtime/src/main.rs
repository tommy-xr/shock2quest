// Debug Runtime - HTTP-controlled game runtime for LLM testing and automation
//
// This runtime provides a localhost-only HTTP API for controlling the game,
// enabling LLMs and automation scripts to test gameplay, debug issues, and
// validate changes without requiring human interaction.

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use cgmath::Vector3;
use clap::Parser;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashSet, net::SocketAddr, time::Duration};
use tokio::{signal, sync::mpsc, sync::oneshot};
use tracing::info;

mod commands;
use commands::*;

// Game engine imports
extern crate glfw;
use self::glfw::{Context, WindowEvent};
use cgmath::{Quaternion, Rotation3, vec2, vec3};
use dark::SCALE_FACTOR;
use engine::{
    EngineRenderContext, profile, scene::Scene, util::compute_view_matrix_from_render_context,
};
use shock2vr::{
    Game, GameOptions, SpawnLocation,
    input::{InputAction, InputActionState},
    input_context::InputContext,
    time::Time,
};

// Property imports for state queries
use dark::properties::{PropModelName, PropPosition, PropSymName, PropTemplateId};
use shipyard::{Get, IntoIter, IntoWithId, View};

// Screen dimensions for the debug window
const SCR_WIDTH: u32 = 800;
const SCR_HEIGHT: u32 = 600;

/// A request-body extractor that parses JSON **regardless of the `Content-Type`
/// header**, unlike axum's `Json`. This debug/test API is driven by ad-hoc
/// clients (`curl -d '{...}'` without a header, the SDK, etc.); requiring
/// `Content-Type: application/json` just produces silent rejections that look
/// like the command did nothing (e.g. `/v1/step` becoming a no-op). An empty
/// body is treated as `{}` so endpoints whose fields are all optional work with
/// no body at all.
struct LenientJson<T>(T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequest<S> for LenientJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("failed to read body: {e}")))?;
        let slice: &[u8] = if bytes.is_empty() { b"{}" } else { &bytes };
        let value = serde_json::from_slice(slice)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))?;
        Ok(LenientJson(value))
    }
}

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

    /// Enable skeleton visualization
    #[arg(long)]
    debug_skeletons: bool,

    /// Enable pathfinding visualization
    #[arg(long)]
    debug_pathfinding: bool,

    /// Save file to load
    #[arg(short, long)]
    save_file: Option<String>,

    /// Enable experimental features (comma-separated)
    #[arg(long)]
    experimental: Option<String>,

    /// Opt into the VR presentation (forearm panels, two-handed interaction).
    /// The debug runtime defaults to flatscreen (screen-space 2D HUD,
    /// first-person viewmodel) to match `desktop_runtime` and because the
    /// flat path is what most headless weapon/aim testing exercises.
    #[arg(long)]
    vr: bool,

    /// Show the game window. By default the runtime creates a hidden window so
    /// it never steals focus or pops to the foreground - rendering and
    /// `/v1/screenshot` still work via the offscreen framebuffer. Pass this to
    /// watch the game interactively.
    #[arg(long)]
    visible: bool,

    /// Opaque instance identifier echoed by /v1/health, so the client that
    /// launched this process can verify it is talking to its own instance
    /// and not another agent's runtime that happens to hold the same port.
    #[arg(long)]
    instance_id: Option<String>,
}

/// Instance identifier from --instance-id, echoed by /v1/health
static INSTANCE_ID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Default debug-camera head rotation.
///
/// Matches the desktop runtime's default view orientation (`cargo dr`): at
/// `yaw=0, pitch=0` the desktop camera forward is `(1,0,0)` fed through
/// `look_at_rh`, so it looks toward `-X`. Mirroring it here keeps debug-runtime
/// screenshots framed the same as what you see interactively on desktop.
///
/// TODO(camera-endpoint): make the debug camera aimable over HTTP - either a new
/// `/v1/camera` endpoint or by honoring the head rotation in `/v1/control/input`
/// - so an agent can point the camera at an arbitrary world point (e.g. wherever
/// a ragdoll lands) instead of relying on this fixed default. Tracked in
/// projects/debug-runtime.md.
/// Fixed simulation timestep used while stepping (`/v1/step`), so frame- and
/// time-based stepping advance a deterministic, wall-clock-independent amount of
/// simulation time (60 Hz, matching the game's target frame rate).
const FIXED_STEP_DT: f32 = 1.0 / 60.0;

/// Head rotation for a yaw/pitch (degrees), matching the desktop runtime's
/// camera convention (`camera_forward` / `camera_rotation`): at `yaw=pitch=0`
/// the forward is `(1,0,0)` fed through `look_at_rh`, i.e. looking toward `-X`.
/// The SAME rotation drives both the render camera and `InputContext.head`, so
/// the flat viewmodel (placed relative to the head) and the rendered view always
/// agree - otherwise the viewmodel renders off-axis as a giant side-on slab.
fn head_rotation_from_yaw_pitch(yaw_deg: f32, pitch_deg: f32) -> Quaternion<f32> {
    use cgmath::{Decomposed, Rotation, Transform, point3};
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let forward = point3(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    );
    let up = vec3(0.0, 1.0, 0.0);
    let decomposed: Decomposed<Vector3<f32>, Quaternion<f32>> =
        Transform::look_at_rh(forward, point3(0.0, 0.0, 0.0), up);
    decomposed.rot.invert()
}

fn default_camera_head_rotation() -> Quaternion<f32> {
    head_rotation_from_yaw_pitch(0.0, 0.0)
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
    // Initialize tracing with info level by default. Include shock2vr so
    // script/trigger activity (tripwires, quest bits, doors) is visible when
    // driving the game over HTTP - essential for agent-driven debugging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug_runtime=info,shock2vr=info".into()),
        )
        .init();

    let args = Args::parse();
    let _ = INSTANCE_ID.set(args.instance_id.clone());

    info!(
        "Starting debug runtime on port {} with mission: {}",
        args.port, args.mission
    );

    // Create a tokio runtime for the HTTP server
    let rt = tokio::runtime::Runtime::new()?;

    // Create command channel for communication between HTTP server and game loop
    let (command_tx, command_rx) = mpsc::unbounded_channel::<RuntimeCommand>();

    // Bind BEFORE starting the game: a taken port (another runtime raced us
    // to it) must be a fast, loud exit - not a headless game loop that a
    // client waits on until its launch timeout.
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = rt
        .block_on(tokio::net::TcpListener::bind(addr))
        .map_err(|e| anyhow::anyhow!("failed to bind {}: {}", addr, e))?;

    // Start the HTTP server in a background task
    let server_handle = rt.spawn(start_http_server(listener, command_tx));

    // Run the game on the main thread (required for GLFW)
    let game_result = run_game_blocking(args, command_rx);

    // If game exits, shutdown the server
    server_handle.abort();

    game_result?;

    Ok(())
}

/// Start the HTTP server on an already-bound listener (binding happens in
/// main so a taken port fails the process fast)
async fn start_http_server(
    listener: tokio::net::TcpListener,
    command_tx: mpsc::UnboundedSender<RuntimeCommand>,
) -> anyhow::Result<()> {
    // Create the router with health endpoint
    let app = Router::new()
        .route("/v1/health", get(health_check))
        .route("/v1/info", get(get_info))
        .route("/v1/step", axum::routing::post(step_frame))
        .route("/v1/shutdown", axum::routing::post(shutdown_server))
        .route("/v1/entities", get(list_entities))
        .route("/v1/entities/:id", get(get_entity_detail))
        .route(
            "/v1/entities/:id/message",
            axum::routing::post(send_entity_message),
        )
        .route("/v1/entities/:id/animation", get(get_animation_state))
        .route("/v1/player/position", get(get_player_position))
        .route("/v1/player/teleport", axum::routing::post(teleport_player))
        .route(
            "/v1/player/move",
            axum::routing::post(move_player_validated),
        )
        .route(
            "/v1/control/transition-level",
            axum::routing::post(transition_level),
        )
        .route("/v1/save", axum::routing::post(save_game))
        .route("/v1/load", axum::routing::post(load_game))
        .route("/v1/ui", get(get_ui_state))
        .route("/v1/quests", get(get_quest_bits))
        .route("/v1/quests/:name", axum::routing::post(set_quest_bit))
        .route("/v1/player/inventory", get(get_player_inventory))
        .route("/v1/transitions", get(list_transitions))
        .route("/v1/player/give", axum::routing::post(give_item))
        .route("/v1/physics/raycast", axum::routing::post(perform_raycast))
        .route("/v1/physics/bodies", get(list_physics_bodies))
        .route("/v1/physics/bodies/:id", get(get_physics_body_detail))
        .route("/v1/physics/joints", get(list_physics_joints))
        .route(
            "/v1/physics/bodies/:id/impulse",
            axum::routing::post(apply_body_impulse),
        )
        .route("/v1/physics/colliders/validate", get(audit_colliders))
        .route("/v1/ragdoll/metrics", get(get_ragdoll_metrics))
        .route("/v1/control/input", get(get_input_state))
        .route("/v1/control/input", axum::routing::post(set_input_channel))
        .route("/v1/control/command", axum::routing::post(run_game_command))
        .route(
            "/v1/pathfinding-test",
            axum::routing::post(pathfinding_test).get(pathfinding_test_status),
        )
        .route("/v1/pathfinding/stats", get(pathfinding_stats))
        .route("/v1/ai/paths", get(ai_paths))
        .route(
            "/v1/input/action",
            axum::routing::post(trigger_input_action),
        )
        .route("/v1/input/actions", get(list_input_actions))
        .route("/v1/audio/recent", get(get_recent_audio))
        .route("/v1/screenshot", axum::routing::post(take_screenshot))
        .with_state(command_tx);

    let addr = listener.local_addr()?;
    info!("Debug runtime listening on http://{}", addr);
    info!(
        "camera eye height (standing): {} SS2 units = {} world units (above pawn) - shared with desktop via PLAYER_EYE_HEIGHT",
        shock2vr::PLAYER_EYE_HEIGHT,
        shock2vr::PLAYER_EYE_HEIGHT / SCALE_FACTOR
    );

    // Log available endpoints
    info!("Available endpoints:");
    info!("  GET  /v1/health           - Health check and server status");
    info!("  GET  /v1/info             - Get current game state snapshot");
    info!("  POST /v1/step             - Step the simulation forward");
    info!("  POST /v1/shutdown         - Shutdown the debug runtime gracefully");
    info!("  GET  /v1/entities         - List entities with optional limit and filter");
    info!("  GET  /v1/entities/{{id}}    - Get detailed entity information");
    info!("  POST /v1/entities/{{id}}/message - Inject a script message (damage/frob/signal)");
    info!(
        "  GET  /v1/entities/{{id}}/animation - Animation playback state + posed skeleton (world-space joints)"
    );
    info!("  GET  /v1/player/position  - Get current player position");
    info!("  POST /v1/player/teleport  - Teleport player to coordinates (raw, unbounded)");
    info!("  POST /v1/player/move      - Bounded, collision-valid move toward {{x,y,z}}");
    info!("  POST /v1/control/transition-level - Warp to another level {{level, loc?}}");
    info!("  POST /v1/save             - Save the game to a named file {{file}}");
    info!(
        "  POST /v1/load             - Load a named save (restores mission/player/quests) {{file}}"
    );
    info!(
        "  GET  /v1/ui               - Flat-mode UI state (mode: shooter/use, MFD panel, inventory strip)"
    );
    info!("  GET  /v1/quests           - Snapshot quest bits (objective flags)");
    info!("  POST /v1/quests/:name     - Set a quest bit {{value: unknown|incomplete|complete}}");
    info!("  GET  /v1/player/inventory - Snapshot the player's carried items");
    info!("  POST /v1/player/give      - Put an existing entity in the inventory {{entity_id}}");
    info!("  POST /v1/physics/raycast  - Perform physics raycast for collision testing");
    info!("  GET  /v1/control/input    - Retrieve controller/input state");
    info!("  POST /v1/control/input    - Update controller/input channels");
    info!("  POST /v1/control/command  - Execute gameplay commands (save, spawn, etc.)");
    info!(
        "  POST /v1/input/action     - Trigger a discrete input action (e.g. PathfindingTestCycle)"
    );
    info!("  GET  /v1/input/actions    - List available input actions");
    info!("  GET  /v1/audio/recent     - Recently played environmental sounds (sample + tags)");
    info!("  POST /v1/screenshot       - Capture the current framebuffer");
    info!("");
    info!("Test with: curl http://{}/v1/health", addr);
    info!("Test with: curl http://{}/v1/info", addr);
    info!("Test with: curl -X POST http://{}/v1/step", addr);
    info!("Test with: curl -X POST http://{}/v1/shutdown", addr);

    // Start the server with graceful shutdown
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

    // Default to a hidden window so the runtime never steals focus or pops to
    // the foreground - it's an HTTP-driven automation tool. A hidden window
    // still has a valid GL context and default framebuffer, so rendering and
    // `/v1/screenshot` work unchanged. `--visible` opts into a normal window.
    glfw.window_hint(glfw::WindowHint::Visible(args.visible));

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
    let bundle_storage = engine.get_storage();
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

    // Flatscreen is the default debug presentation (matching desktop_runtime);
    // --vr opts into the VR forearm/hands path.
    let presentation_mode = if args.vr {
        shock2vr::PresentationMode::Vr
    } else {
        shock2vr::PresentationMode::Flat
    };
    info!("Presentation mode: {:?}", presentation_mode);

    let options = GameOptions {
        mission: mission.clone(),
        presentation_mode,
        spawn_location,
        save_file: args.save_file,
        debug_draw: args.debug_draw,
        debug_physics: args.debug_physics,
        debug_portals: args.debug_portals,
        debug_show_ids: args.debug_show_ids,
        debug_skeletons: args.debug_skeletons,
        debug_pathfinding: args.debug_pathfinding,
        debug_ai: false,
        render_particles: true,
        experimental_features,
        ..GameOptions::default()
    };

    info!("Step 6: Initializing game with mission: {}", mission);

    let mut game = Game::init(options, bundle_storage);

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
    let mut action_state = InputActionState::new();

    // Persistent input state, patched over HTTP via `/v1/control/input` and fed
    // to `game.update` each frame. Unlike discrete actions (consumed once),
    // these are level-held values (trigger held down, head aimed somewhere), so
    // they must persist across frames rather than be rebuilt to zero each frame.
    // The head starts at the desktop default view; the SAME head rotation also
    // drives the render camera (below), so the flat viewmodel stays aligned with
    // what is rendered.
    let mut current_input = InputContext::default();
    current_input.head.rotation = default_camera_head_rotation();
    // In VR mode, give the simulated hands a natural first-person rest pose
    // (pawn-local; forward is -X, matching the desktop camera convention) so
    // `--vr` shows the glove hands at the bottom of the view without any
    // /v1/control/input setup. The yaw-90 rotation aims each hand's -Z
    // (raycast/fingers) at pawn-forward, like desktop_runtime's default hand
    // yaw. HTTP patches override these as usual.
    if args.vr {
        let aim_forward = Quaternion::from_angle_y(cgmath::Deg(90.0));
        current_input.right_hand.position = vec3(-0.55, 1.4, -0.2);
        current_input.right_hand.rotation = aim_forward;
        current_input.left_hand.position = vec3(-0.55, 1.4, 0.2);
        current_input.left_hand.rotation = aim_forward;
    }

    // Deferred replies so HTTP commands observe a complete, post-render frame:
    // - `Step` replies only after all requested frames have actually run, so
    //   `/v1/step` blocks until stepping is done (otherwise screenshots/queries
    //   race the still-running step and read a stale frame).
    // - `Screenshot` captures after the frame is fully rendered (below), not at
    //   command-receive time (which is before this iteration's render/swap and
    //   would read a stale/blank back buffer -> intermittent black screenshots).
    let mut pending_step_reply: Option<oneshot::Sender<Result<StepResult, StepError>>> = None;
    let mut frames_advanced_this_step = 0u32;
    let mut pending_screenshots: Vec<(ScreenshotSpec, oneshot::Sender<ScreenshotResult>)> =
        Vec::new();

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

        let game_time = Time {
            elapsed: Duration::from_secs_f32(delta_time),
            total: Duration::from_secs_f32(time - start_time),
        };

        // Process commands from HTTP server. Step and Screenshot replies are
        // deferred (see below) so callers observe a complete, post-render frame;
        // every other command is handled synchronously here.
        while let Ok(command) = command_rx.try_recv() {
            match command {
                RuntimeCommand::Step(step_spec, reply) => {
                    if pending_step_reply.is_some() {
                        tracing::warn!(
                            "Rejected Step command - another step is already in progress"
                        );
                        let _ = reply.send(Err(StepError::AlreadyInProgress));
                        continue;
                    }

                    match step_spec {
                        StepSpec::Frames { frames } => {
                            frames_to_step = frames;
                            target_step_time = None;
                            tracing::info!("Starting step: {} frames", frames);
                        }
                        StepSpec::Duration { duration } => {
                            match duration.parse::<humantime::Duration>() {
                                Ok(parsed_duration) => {
                                    let duration_secs = parsed_duration.as_secs_f32();
                                    target_step_time = Some(accumulated_time + duration_secs);
                                    frames_to_step = 0;
                                    tracing::info!(
                                        "Starting step: {} ({:.3}s)",
                                        duration,
                                        duration_secs
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to parse duration '{}': {}",
                                        duration,
                                        e
                                    );
                                    // Nothing to step; reply immediately so the
                                    // caller isn't left hanging.
                                    let _ = reply.send(Ok(StepResult {
                                        frames_advanced: 0,
                                        time_advanced: 0.0,
                                        new_frame_index: frame_counter,
                                        new_total_time: accumulated_time,
                                    }));
                                    continue;
                                }
                            }
                        }
                    }
                    step_requested = true;
                    is_paused = false;
                    frames_advanced_this_step = 0;
                    // Reply is sent once stepping actually completes (below).
                    pending_step_reply = Some(reply);
                }
                RuntimeCommand::Screenshot(spec, reply) => {
                    // Captured after this frame finishes rendering (below).
                    pending_screenshots.push((spec, reply));
                }
                RuntimeCommand::Shutdown => {
                    shutdown_requested = true;
                    tracing::info!("Shutdown requested via API");
                }
                other => {
                    process_command(
                        other,
                        &mut game,
                        &game_time,
                        frame_counter,
                        &mut action_state,
                        &mut current_input,
                    );
                }
            }
        }

        // Only update the game if not paused or if step was requested
        let actual_game_time = if !is_paused || step_requested {
            // Deterministic stepping: while stepping (frame- or time-based) advance
            // the simulation by a FIXED timestep rather than the wall-clock dt
            // between HTTP requests. Real dt makes `{frames:N}` non-deterministic
            // and lets physics (e.g. a settling ragdoll) lurch in erratic slow-
            // motion. With a fixed step, `{frames:N}` == N/FPS seconds of sim time
            // and `{duration:T}` runs exactly T/dt frames. Free-running (not
            // stepping) still uses real wall-clock dt.
            let game_time = if step_requested {
                Time {
                    elapsed: Duration::from_secs_f32(FIXED_STEP_DT),
                    total: Duration::from_secs_f32(accumulated_time + FIXED_STEP_DT),
                }
            } else {
                game_time.clone()
            };
            profile!(
                "game.update",
                game.update(&game_time, &current_input, &mut action_state)
            );

            if step_requested {
                // Increment frame counter and accumulated time
                frame_counter += 1;
                frames_advanced_this_step += 1;
                accumulated_time += game_time.elapsed.as_secs_f32();

                // Check if we should continue stepping or pause
                let should_continue = if let Some(target_time) = target_step_time {
                    // Time-based stepping
                    if accumulated_time >= target_time {
                        tracing::info!(
                            "Time-based step completed: reached {:.3}s after {} frames",
                            accumulated_time,
                            frame_counter
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
                            frame_counter,
                            accumulated_time
                        );
                        false
                    } else {
                        true
                    }
                } else {
                    // Single frame step (legacy behavior)
                    tracing::info!(
                        "Stepped 1 frame, game paused again. Frame: {}, Total time: {:.3}s",
                        frame_counter,
                        accumulated_time
                    );
                    false
                };

                if !should_continue {
                    step_requested = false;
                    is_paused = true;
                    target_step_time = None;
                    frames_to_step = 0;
                    // Stepping finished: now answer the /v1/step caller. (The final
                    // frame is rendered later this same loop iteration, so by the
                    // time the HTTP response is observed the frame is up to date.)
                    if let Some(reply) = pending_step_reply.take() {
                        let _ = reply.send(Ok(StepResult {
                            frames_advanced: frames_advanced_this_step,
                            time_advanced: frames_advanced_this_step as f32 * FIXED_STEP_DT,
                            new_frame_index: frame_counter,
                            new_total_time: accumulated_time,
                        }));
                    }
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
                game.update(&zero_time, &current_input, &mut action_state)
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
            // Crouch-aware eye height, shared with desktop and the flat
            // controller via Game::player_eye_height, so the debug-runtime
            // camera sits at the same height as desktop and shots land on the
            // crosshair.
            head_offset: vec3(0.0, game.player_eye_height() / SCALE_FACTOR, 0.0),
            // Same head rotation fed to game.update, so the rendered view and the
            // flat viewmodel agree. Controllable via `/v1/control/input` head.look.
            head_rotation: current_input.head.rotation,
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

        // Service deferred screenshots now that the frame is fully drawn to the
        // back buffer (before the swap, so we read the just-rendered contents).
        if !pending_screenshots.is_empty() {
            for (spec, reply) in pending_screenshots.drain(..) {
                let result = capture_screenshot_to_result(spec);
                if reply.send(result).is_err() {
                    tracing::warn!("Failed to send screenshot result - receiver dropped");
                }
            }
        }

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
fn process_command(
    command: RuntimeCommand,
    game: &mut Game,
    time: &Time,
    frame_counter: u64,
    action_state: &mut InputActionState,
    current_input: &mut InputContext,
) {
    match command {
        RuntimeCommand::GetInfo(reply) => {
            let snapshot = capture_frame_snapshot(game, time, frame_counter);
            if let Err(_) = reply.send(snapshot) {
                tracing::warn!("Failed to send frame snapshot - receiver dropped");
            }
        }
        // Step and Screenshot are intercepted in the game loop (deferred replies),
        // so they never reach process_command.
        RuntimeCommand::Step(..) | RuntimeCommand::Screenshot(..) => {
            unreachable!("Step/Screenshot are handled in the game loop")
        }
        RuntimeCommand::RayCast(request, reply) => {
            let result = if let Some(debug_scene) = game.debug_scene() {
                use cgmath::{InnerSpace, Point3};
                use shock2vr::game_scene::RaycastMask;

                // Convert request to raycast parameters
                let start = Point3::new(request.start[0], request.start[1], request.start[2]);
                let mut end = Point3::new(request.end[0], request.end[1], request.end[2]);

                if let Some(max_distance) = request.max_distance {
                    if max_distance > 0.0 {
                        let direction = end - start;
                        let length = direction.magnitude();
                        if length > 0.0 {
                            let clamped = length.min(max_distance);
                            let normalized = direction / length;
                            end = start + normalized * clamped;
                        }
                    }
                }

                let mask = RaycastMask {
                    groups: request
                        .collision_groups
                        .unwrap_or_else(|| vec!["entity".to_string(), "level".to_string()]),
                };

                // Perform the raycast
                let hit = debug_scene.raycast(start, end, mask);

                // Convert result
                RayCastResult {
                    hit: hit.hit,
                    hit_point: hit.hit_point,
                    hit_normal: hit.hit_normal,
                    distance: hit.distance,
                    entity_id: hit.entity_id,
                    entity_name: hit.entity_name,
                    collision_group: hit.collision_group,
                    is_sensor: hit.is_sensor,
                }
            } else {
                tracing::error!("No debug scene available for raycast");
                RayCastResult {
                    hit: false,
                    hit_point: None,
                    hit_normal: None,
                    distance: None,
                    entity_id: None,
                    entity_name: None,
                    collision_group: None,
                    is_sensor: false,
                }
            };

            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send raycast result - receiver dropped");
            }
        }
        RuntimeCommand::MovePlayer(position) => {
            tracing::info!("Teleporting player to position: {:?}", position);
            if let Some(debug_scene) = game.debug_scene_mut() {
                match debug_scene.teleport_player(position) {
                    Ok(()) => {
                        tracing::info!("Player teleported successfully to {:?}", position);
                    }
                    Err(e) => {
                        tracing::error!("Failed to teleport player: {}", e);
                    }
                }
            } else {
                tracing::error!("No debuggable scene available for player movement");
            }
        }
        RuntimeCommand::MovePlayerValidated { target, reply } => {
            tracing::info!("Validated player move toward: {:?}", target);
            let result = if let Some(debug_scene) = game.debug_scene_mut() {
                let r = debug_scene.move_player(target);
                MoveResult {
                    moved: r.moved,
                    blocked: r.blocked,
                    new_position: [r.new_position.x, r.new_position.y, r.new_position.z],
                    distance_moved: r.distance_moved,
                    requested_distance: r.requested_distance,
                }
            } else {
                tracing::error!("No debuggable scene available for validated player move");
                // No scene: report a no-op move at the origin so the HTTP layer
                // still gets a well-formed response.
                MoveResult {
                    moved: false,
                    blocked: false,
                    new_position: [0.0, 0.0, 0.0],
                    distance_moved: 0.0,
                    requested_distance: 0.0,
                }
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send validated-move result - receiver dropped");
            }
        }
        RuntimeCommand::TransitionLevel {
            level_file,
            loc,
            reply,
        } => {
            tracing::info!("Transitioning level to {} (loc {:?})", level_file, loc);
            game.transition_level(level_file.clone(), loc);
            // Report the ACTUAL post-switch scene rather than assuming success.
            // Without the loading_screen feature the switch is synchronous and
            // scene_name() is already the target; with it the switch is deferred
            // (scene_name() is still "loading"/the old level), so success stays
            // false until the caller steps far enough for it to complete.
            let mission = game.scene_name().to_string();
            let success = mission == level_file;
            let message = if success {
                format!("Transitioned to {}", mission)
            } else {
                format!(
                    "Transition to {} queued (current scene: {})",
                    level_file, mission
                )
            };
            let result = commands::TransitionLevelResult {
                success,
                mission,
                message,
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send transition result - receiver dropped");
            }
        }
        RuntimeCommand::SaveGame { file, reply } => {
            tracing::info!("Saving game to '{}'", file);
            // The underlying save path unwraps on I/O errors and on scenes that
            // lack a player/quest state (e.g. a debug scene). Contain a panic
            // here so a failed save returns an error instead of unwinding out of
            // the game-loop thread and bricking the runtime for every later
            // command. A save is `&self`, so a caught panic leaves state intact.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                game.save_game(file.clone())
            }));
            let result = match outcome {
                Ok(mission) => commands::SaveLoadResult {
                    success: true,
                    message: format!("Saved '{}' ({})", file, mission),
                    file,
                    mission,
                },
                Err(_) => {
                    tracing::error!("Save of '{}' panicked (caught to keep runtime alive)", file);
                    commands::SaveLoadResult {
                        success: false,
                        message: format!("Failed to save '{}' (see runtime log)", file),
                        file,
                        mission: String::new(),
                    }
                }
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send save result - receiver dropped");
            }
        }
        RuntimeCommand::LoadGame { file, reply } => {
            tracing::info!("Loading game from '{}'", file);
            // Existence is pre-checked in the handler, but a file that exists yet
            // is truncated / not UTF-8 / an incompatible save schema still panics
            // inside SaveData::read. Contain it so a corrupt save returns an error
            // rather than bricking the game-loop thread. load_from_file only swaps
            // `active_game_scene` as its final step (after all fallible reads), so
            // a caught panic leaves the previously-active scene intact.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                game.load_game(file.clone())
            }));
            let result = match outcome {
                Ok(mission) => commands::SaveLoadResult {
                    success: true,
                    message: format!("Loaded '{}' ({})", file, mission),
                    file,
                    mission,
                },
                Err(_) => {
                    tracing::error!("Load of '{}' panicked (caught to keep runtime alive)", file);
                    commands::SaveLoadResult {
                        success: false,
                        message: format!(
                            "Failed to load '{}' - corrupt or incompatible save",
                            file
                        ),
                        file,
                        mission: String::new(),
                    }
                }
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send load result - receiver dropped");
            }
        }
        RuntimeCommand::GetUiState { reply } => {
            let result = game
                .debug_scene()
                .map(|scene| {
                    let ui = scene.ui_state();
                    commands::UiStateResult {
                        mode: ui.mode,
                        active_panel: ui.active_panel,
                        strip: ui.strip,
                        cursor: ui.cursor,
                        ammo_cycle: ui.ammo_cycle,
                    }
                })
                .unwrap_or(commands::UiStateResult {
                    mode: "shooter".to_string(),
                    active_panel: None,
                    strip: None,
                    cursor: None,
                    ammo_cycle: None,
                });
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send ui state - receiver dropped");
            }
        }
        RuntimeCommand::GetQuestBits { reply } => {
            let quests: Vec<commands::QuestBitEntry> = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .quest_bits()
                        .into_iter()
                        .map(|q| commands::QuestBitEntry {
                            name: q.name,
                            value: q.value,
                            bits: q.bits,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let result = commands::QuestBitsResult {
                count: quests.len(),
                quests,
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send quest bits - receiver dropped");
            }
        }
        RuntimeCommand::SetQuestBit { name, value, reply } => {
            let result = match game.debug_scene_mut() {
                Some(scene) => scene.set_quest_bit(&name, &value),
                None => Err("no debuggable scene available".to_string()),
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send set-quest-bit result - receiver dropped");
            }
        }
        RuntimeCommand::GetPlayerInventory { reply } => {
            let items: Vec<commands::InventoryItemEntry> = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .player_inventory()
                        .into_iter()
                        .map(|i| commands::InventoryItemEntry {
                            entity_id: i.entity_id,
                            name: i.name,
                            location: i.location,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let result = commands::PlayerInventoryResult {
                count: items.len(),
                items,
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send player inventory - receiver dropped");
            }
        }
        RuntimeCommand::ListTransitions { reply } => {
            let transitions: Vec<commands::TransitionEntry> = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .list_transitions()
                        .into_iter()
                        .map(|t| commands::TransitionEntry {
                            entity_id: t.entity_id,
                            name: t.name,
                            dest_level: t.dest_level,
                            dest_loc: t.dest_loc,
                            position: t.position,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let result = commands::TransitionsResult {
                count: transitions.len(),
                transitions,
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send transitions - receiver dropped");
            }
        }
        RuntimeCommand::GiveItem { entity_id, reply } => {
            // Client-supplied ids are `EntityId::inner() as i32` (generation
            // bits truncated); resolve against the live entities to recover
            // the full id (see SendEntityMessage).
            let result = match game.debug_scene_mut() {
                Some(scene) => match scene.resolve_entity_id(entity_id) {
                    Some(eid) => scene.give_item(eid),
                    None => Err(format!("invalid entity id {}", entity_id)),
                },
                None => Err("no debuggable scene available".to_string()),
            };
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send give-item result - receiver dropped");
            }
        }
        RuntimeCommand::GetPlayerPosition(reply) => {
            if let Some(debug_scene) = game.debug_scene() {
                let position = debug_scene.player_position();
                tracing::debug!("Retrieved player position: {:?}", position);
                if let Err(_) = reply.send(position) {
                    tracing::warn!("Failed to send player position - receiver dropped");
                }
            } else {
                tracing::error!("No debuggable scene available for getting player position");
                if let Err(_) = reply.send(Vector3::new(0.0, 0.0, 0.0)) {
                    tracing::warn!("Failed to send default player position - receiver dropped");
                }
            }
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
        RuntimeCommand::PathfindingTest(action, reply) => {
            // "cycle" matches the desktop P key behavior; it is injected as an
            // input action and consumed by the next game update.
            let result = if action == "cycle" {
                action_state.trigger(InputAction::PathfindingTestCycle);
                action_state.release(InputAction::PathfindingTestCycle);
                CommandResult {
                    success: true,
                    message: "Pathfinding test cycle triggered - applies on next update"
                        .to_string(),
                    data: None,
                }
            } else {
                CommandResult {
                    success: false,
                    message: format!(
                        "Unsupported pathfinding test action '{}' - only 'cycle' is currently supported (matches the desktop P key)",
                        action
                    ),
                    data: None,
                }
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send pathfinding test result - receiver dropped");
            }
        }
        RuntimeCommand::TriggerAction(action, reply) => {
            action_state.trigger(action);
            // Single-shot semantics: don't leave the action held
            action_state.release(action);
            let result = CommandResult {
                success: true,
                message: format!("Triggered action '{}' - applies on next update", action),
                data: None,
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send trigger action result - receiver dropped");
            }
        }
        RuntimeCommand::GetPathfindingTestStatus(reply) => {
            let result = if let Some(debug_scene) = game.debug_scene() {
                let status = debug_scene.pathfinding_test_status();
                PathfindingTestStatusResult {
                    state: status.state,
                    test_path_waypoints: status.test_path_waypoints,
                }
            } else {
                PathfindingTestStatusResult {
                    state: "Unavailable".to_string(),
                    test_path_waypoints: 0,
                }
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send pathfinding test status - receiver dropped");
            }
        }
        RuntimeCommand::GetPathfindingStats(reply) => {
            let stats = game
                .debug_scene()
                .and_then(|debug_scene| debug_scene.pathfinding_stats());
            if reply.send(stats).is_err() {
                tracing::warn!("Failed to send pathfinding stats - receiver dropped");
            }
        }
        RuntimeCommand::GetAiPaths(reply) => {
            let paths = game
                .debug_scene()
                .map(|debug_scene| debug_scene.ai_paths())
                .unwrap_or_default();
            if reply.send(paths).is_err() {
                tracing::warn!("Failed to send AI paths - receiver dropped");
            }
        }
        RuntimeCommand::ListEntities {
            limit,
            filter,
            reply,
        } => {
            if let Some(debug_scene) = game.debug_scene() {
                let entities = debug_scene.list_entities(limit, filter.as_deref());
                let player_pos = debug_scene.player_position();
                let result = EntityListResult {
                    total_count: entities.len(),
                    player_position: [player_pos.x, player_pos.y, player_pos.z],
                    entities: entities
                        .into_iter()
                        .map(|e| EntitySummary {
                            id: e.id,
                            name: e.name,
                            template_id: e.template_id,
                            position: e.position,
                            distance: e.distance,
                            script_count: e.script_count,
                            link_count: e.link_count,
                        })
                        .collect(),
                };
                if let Err(_) = reply.send(result) {
                    tracing::warn!("Failed to send entity list - receiver dropped");
                }
            } else {
                let result = EntityListResult {
                    entities: vec![],
                    total_count: 0,
                    player_position: [0.0, 0.0, 0.0],
                };
                if let Err(_) = reply.send(result) {
                    tracing::warn!("No debug scene available");
                }
            }
        }
        RuntimeCommand::EntityDetail { id, reply } => {
            let result = if let Some(debug_scene) = game.debug_scene() {
                // `id` is `EntityId::inner() as i32` from the list endpoint,
                // which drops the generation bits - resolve against the live
                // entities to recover the full id (see SendEntityMessage).
                debug_scene
                    .resolve_entity_id(id)
                    .and_then(|entity_id| debug_scene.entity_detail(entity_id))
                    .map(|detail| EntityDetailResult {
                        entity_id: detail.entity_id,
                        name: detail.name,
                        template_id: detail.template_id,
                        position: detail.position,
                        rotation: detail.rotation,
                        inheritance_chain: detail.inheritance_chain,
                        properties: detail
                            .properties
                            .into_iter()
                            .map(|p| PropertyInfo {
                                name: p.name,
                                value: p.value,
                            })
                            .collect(),
                        outgoing_links: detail
                            .outgoing_links
                            .into_iter()
                            .map(|l| LinkInfo {
                                link_type: l.link_type,
                                target_id: l.target_id,
                                target_name: l.target_name,
                            })
                            .collect(),
                        incoming_links: detail
                            .incoming_links
                            .into_iter()
                            .map(|l| LinkInfo {
                                link_type: l.link_type,
                                target_id: l.target_id,
                                target_name: l.target_name,
                            })
                            .collect(),
                    })
            } else {
                None
            };

            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send entity detail - receiver dropped");
            }
        }
        RuntimeCommand::AnimationState { id, reply } => {
            let result = game.debug_scene().and_then(|debug_scene| {
                // Same id space as /v1/entities (`EntityId::inner() as i32`).
                debug_scene
                    .resolve_entity_id(id)
                    .and_then(|entity_id| debug_scene.animation_state(entity_id))
            });

            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send animation state - receiver dropped");
            }
        }
        RuntimeCommand::SendEntityMessage { id, message, reply } => {
            // The list/detail endpoints expose `EntityId::inner() as i32`,
            // which truncates the generation bits: `from_inner(id as u64)`
            // would rebuild a generation-0 handle that is stale for any
            // recycled slot (issue #484). Resolve against the live entities
            // instead, which recovers the full id by index.
            let result = match game.debug_scene_mut() {
                Some(debug_scene) => match debug_scene.resolve_entity_id(id) {
                    Some(entity_id) => {
                        let queued = debug_scene.send_entity_message(entity_id, message);
                        CommandResult {
                            success: queued,
                            message: if queued {
                                format!("Message queued for entity {}", id)
                            } else {
                                format!("Entity {} not found or not alive", id)
                            },
                            data: None,
                        }
                    }
                    None => CommandResult {
                        success: false,
                        message: format!("Entity {} not found or not alive", id),
                        data: None,
                    },
                },
                None => CommandResult {
                    success: false,
                    message: "No debuggable scene available".to_string(),
                    data: None,
                },
            };

            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send entity message result - receiver dropped");
            }
        }
        RuntimeCommand::ListPhysicsBodies {
            limit,
            entity_id,
            reply,
        } => {
            if let Some(debug_scene) = game.debug_scene() {
                // Fetch all bodies (limit applied after filtering), then scope to
                // the requested entity id if one was provided.
                let mut bodies = debug_scene.list_physics_bodies(None);
                if let Some(entity_id) = entity_id {
                    bodies.retain(|b| b.entity_id == Some(entity_id));
                }
                let total_count = bodies.len();
                if let Some(limit) = limit {
                    bodies.truncate(limit);
                }
                let player_pos = debug_scene.player_position();
                let result = PhysicsBodyListResult {
                    total_count,
                    player_position: [player_pos.x, player_pos.y, player_pos.z],
                    bodies: bodies
                        .into_iter()
                        .map(|b| PhysicsBodySummary {
                            body_id: b.body_id,
                            entity_id: b.entity_id,
                            entity_name: b.entity_name,
                            body_type: b.body_type,
                            position: b.position,
                            rotation: b.rotation,
                            mass: b.mass,
                            velocity: b.velocity,
                            angular_velocity: b.angular_velocity,
                            collision_groups: b.collision_groups,
                            is_sensor: b.is_sensor,
                            is_enabled: b.is_enabled,
                        })
                        .collect(),
                };
                if let Err(_) = reply.send(result) {
                    tracing::warn!("Failed to send physics body list - receiver dropped");
                }
            } else {
                let result = PhysicsBodyListResult {
                    bodies: vec![],
                    total_count: 0,
                    player_position: [0.0, 0.0, 0.0],
                };
                if let Err(_) = reply.send(result) {
                    tracing::warn!("No debug scene available for physics body listing");
                }
            }
        }
        RuntimeCommand::PhysicsBodyDetail { id, reply } => {
            let result = if let Some(debug_scene) = game.debug_scene() {
                debug_scene
                    .physics_body_detail(id)
                    .map(|detail| PhysicsBodyDetailResult {
                        body_id: detail.body_id,
                        entity_id: detail.entity_id,
                        entity_name: detail.entity_name,
                        body_type: detail.body_type,
                        position: detail.position,
                        rotation: detail.rotation,
                        linear_velocity: detail.linear_velocity,
                        angular_velocity: detail.angular_velocity,
                        mass: detail.mass,
                        center_of_mass: detail.center_of_mass,
                        moment_of_inertia: detail.moment_of_inertia,
                        gravity_scale: detail.gravity_scale,
                        linear_damping: detail.linear_damping,
                        angular_damping: detail.angular_damping,
                        collision_groups: detail.collision_groups,
                        is_sensor: detail.is_sensor,
                        is_enabled: detail.is_enabled,
                        is_sleeping: detail.is_sleeping,
                        contact_count: detail.contact_count,
                    })
            } else {
                None
            };

            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send physics body detail - receiver dropped");
            }
        }
        RuntimeCommand::RagdollMetrics { reply } => {
            let ragdolls = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .ragdoll_metrics()
                        .into_iter()
                        .map(|m| commands::RagdollMetricsEntry {
                            entity_id: m.entity_id,
                            body_count: m.body_count,
                            max_linear_speed: m.max_linear_speed,
                            max_angular_speed: m.max_angular_speed,
                            min_y: m.min_y,
                            max_nonadjacent_overlap: m.max_nonadjacent_overlap,
                            max_drift: m.max_drift,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Err(_) = reply.send(commands::RagdollMetricsResult { ragdolls }) {
                tracing::warn!("Failed to send ragdoll metrics - receiver dropped");
            }
        }
        RuntimeCommand::AuditColliders { reply } => {
            let issues: Vec<commands::ColliderIssueEntry> = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .audit_colliders()
                        .into_iter()
                        .map(|iss| commands::ColliderIssueEntry {
                            entity_id: iss.entity_id,
                            entity_name: iss.entity_name,
                            kind: iss.kind,
                            aabb_min: iss.aabb_min,
                            aabb_max: iss.aabb_max,
                            is_sensor: iss.is_sensor,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if reply
                .send(commands::ColliderAuditResult {
                    total_count: issues.len(),
                    issues,
                })
                .is_err()
            {
                tracing::warn!("Failed to send collider audit - receiver dropped");
            }
        }
        RuntimeCommand::ListPhysicsJoints { reply } => {
            let joints = game
                .debug_scene()
                .map(|scene| {
                    scene
                        .list_physics_joints()
                        .into_iter()
                        .map(|j| commands::PhysicsJointEntry {
                            body1_id: j.body1_id,
                            body2_id: j.body2_id,
                            joint_type: j.joint_type,
                            bone1: j.bone1,
                            bone2: j.bone2,
                            anchor1: j.anchor1,
                            anchor2: j.anchor2,
                            separation: j.separation,
                            linear_impulse: j.linear_impulse,
                            angular_impulse: j.angular_impulse,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Err(_) = reply.send(commands::PhysicsJointsResult { joints }) {
                tracing::warn!("Failed to send physics joints - receiver dropped");
            }
        }
        RuntimeCommand::ApplyBodyImpulse {
            body_id,
            impulse,
            reply,
        } => {
            let result = match game.debug_scene_mut() {
                Some(debug_scene) => {
                    let applied = debug_scene.apply_body_impulse(body_id, impulse);
                    CommandResult {
                        success: applied,
                        message: if applied {
                            format!("Impulse applied to body {}", body_id)
                        } else {
                            format!("No dynamic body with id {}", body_id)
                        },
                        data: None,
                    }
                }
                None => CommandResult {
                    success: false,
                    message: "Current scene is not debuggable".to_string(),
                    data: None,
                },
            };
            if let Err(_) = reply.send(result) {
                tracing::warn!("Failed to send impulse result - receiver dropped");
            }
        }
        RuntimeCommand::GetInput(reply) => {
            // Report the runtime-owned input state (what is fed to game.update).
            let input_state = input_state_from_context(current_input);
            if let Err(_) = reply.send(input_state) {
                tracing::warn!("Failed to send input state - receiver dropped");
            }
        }
        RuntimeCommand::SetInput(patches, reply) => {
            // Patch the runtime-owned input state directly; it is fed to
            // game.update each frame and persists until changed. Apply all
            // patches, stopping at the first invalid channel/value so the HTTP
            // caller gets an actionable error instead of a silent partial apply.
            let mut result = Ok(());
            for patch in &patches {
                match apply_input_patch(current_input, &patch.channel, &patch.value) {
                    Ok(()) => tracing::info!(
                        "Set input channel '{}' = {} via remote control",
                        patch.channel,
                        patch.value
                    ),
                    Err(msg) => {
                        tracing::warn!("Rejected input patch: {}", msg);
                        result = Err(msg);
                        break;
                    }
                }
            }
            if reply.send(result).is_err() {
                tracing::warn!("Failed to send SetInput result - receiver dropped");
            }
        }
        RuntimeCommand::Shutdown => {
            // Shutdown is handled in the main loop, this is just for completeness
            tracing::info!("Processing shutdown command");
        }
    }
}

/// Build the serializable `InputState` reported by `GET /v1/control/input`.
fn input_state_from_context(input: &InputContext) -> commands::InputState {
    fn hand(h: &shock2vr::input_context::Hand) -> commands::InputHand {
        commands::InputHand {
            position: [h.position.x, h.position.y, h.position.z],
            rotation: [h.rotation.v.x, h.rotation.v.y, h.rotation.v.z, h.rotation.s],
            thumbstick: [h.thumbstick.x, h.thumbstick.y],
            trigger_value: h.trigger_value,
            squeeze_value: h.squeeze_value,
            a_value: h.a_value,
        }
    }
    commands::InputState {
        pointer: input.pointer.map(|p| commands::InputPointer {
            position: [p.position.x, p.position.y],
            pressed: p.pressed,
        }),
        head: commands::InputHead {
            rotation: [
                input.head.rotation.v.x,
                input.head.rotation.v.y,
                input.head.rotation.v.z,
                input.head.rotation.s,
            ],
        },
        left_hand: hand(&input.left_hand),
        right_hand: hand(&input.right_hand),
        crouch: input.crouch,
    }
}

/// Patch a single channel of the runtime-owned input state, returning false for
/// an unrecognized channel or a value of the wrong shape. These persist across
/// frames (a held trigger, a fixed aim), unlike the edge-triggered actions of
/// `/v1/input/action`.
///
/// Recognized channels:
/// - `head.rotation`                : `[x, y, z, w]` quaternion
/// - `head.look`                    : `[yaw_deg, pitch_deg]` convenience (forward = -Z)
/// - `{left,right}_hand.trigger`    : number in [0, 1] (alias `trigger_value`)
/// - `{left,right}_hand.squeeze`    : number in [0, 1] (alias `squeeze_value`)
/// - `{left,right}_hand.a`          : number in [0, 1] (alias `a_value`)
/// - `{left,right}_hand.thumbstick` : `[x, y]`
/// One line describing every recognized input channel, used in error messages so
/// a bad request is self-documenting. Includes the locomotion semantics (which
/// stick does what) since that is the game's convention, not guessable.
fn input_channels_help() -> &'static str {
    "valid channels: head.rotation [x,y,z,w], head.look [yaw_deg,pitch_deg], \
     pointer.position [x,y] in [0,1] (origin top-left; null clears), pointer.pressed 0|1, \
     {left,right}_hand.{trigger,squeeze,a} <number 0..1>, \
     {left,right}_hand.thumbstick [x,y], \
     {left,right}_hand.position [x,y,z] (pawn-local), \
     {left,right}_hand.rotation [x,y,z,w], \
     crouch 0|1 (stand-up refused without headroom); \
     locomotion: right_hand.thumbstick [strafe, forward] moves the player, \
     left_hand.thumbstick.x turns, left_hand.thumbstick.y flies up/down"
}

fn apply_input_patch(input: &mut InputContext, channel: &str, value: &Value) -> Result<(), String> {
    use cgmath::Vector2;

    // Parse a scalar/array value for `channel`, attributing a clear error to the
    // channel and value shape when it doesn't match.
    fn num(channel: &str, v: &Value) -> Result<f32, String> {
        v.as_f64()
            .map(|f| f as f32)
            .ok_or_else(|| format!("channel '{channel}' expects a number, got {v}"))
    }
    fn arr(channel: &str, v: &Value, n: usize) -> Result<Vec<f32>, String> {
        let a = v.as_array().ok_or_else(|| {
            format!("channel '{channel}' expects an array of {n} numbers, got {v}")
        })?;
        if a.len() != n {
            return Err(format!(
                "channel '{channel}' expects {n} numbers, got {} ({v})",
                a.len()
            ));
        }
        a.iter()
            .map(|e| num(channel, e))
            .collect::<Result<Vec<_>, _>>()
    }

    // Hand channels: "<left|right>_hand.<field>"
    if let Some((side, field)) = channel.split_once("_hand.") {
        let hand = match side {
            "left" => &mut input.left_hand,
            "right" => &mut input.right_hand,
            _ => {
                return Err(format!(
                    "unknown input channel '{channel}'; {}",
                    input_channels_help()
                ));
            }
        };
        return match field {
            "trigger" | "trigger_value" => {
                hand.trigger_value = num(channel, value)?;
                Ok(())
            }
            "squeeze" | "squeeze_value" => {
                hand.squeeze_value = num(channel, value)?;
                Ok(())
            }
            "a" | "a_value" => {
                hand.a_value = num(channel, value)?;
                Ok(())
            }
            "thumbstick" => {
                let a = arr(channel, value, 2)?;
                hand.thumbstick = Vector2::new(a[0], a[1]);
                Ok(())
            }
            "position" => {
                let p = arr(channel, value, 3)?;
                hand.position = cgmath::vec3(p[0], p[1], p[2]);
                Ok(())
            }
            "rotation" => {
                let q = arr(channel, value, 4)?;
                hand.rotation =
                    cgmath::InnerSpace::normalize(Quaternion::new(q[3], q[0], q[1], q[2]));
                Ok(())
            }
            _ => Err(format!(
                "unknown input channel '{channel}'; {}",
                input_channels_help()
            )),
        };
    }

    match channel {
        "head.rotation" => {
            let q = arr(channel, value, 4)?;
            input.head.rotation = Quaternion::new(q[3], q[0], q[1], q[2]);
            Ok(())
        }
        // Desktop camera convention (yaw=pitch=0 looks toward -X); drives both
        // the render camera and the viewmodel. Convenient for pointing the
        // camera without hand-authoring a quaternion.
        "head.look" => {
            let yp = arr(channel, value, 2)?;
            input.head.rotation = head_rotation_from_yaw_pitch(yp[0], yp[1]);
            Ok(())
        }
        // Flat-mode 2D pointer (cursor). Position is normalized [0,1] per
        // axis, origin top-left; setting either channel materializes the
        // pointer (it is `None` until first set). `pointer.position: null`
        // clears the pointer back to `None` (scenes branch on Some/None, so
        // the cleared state must be reachable for testing).
        "pointer.position" => {
            if value.is_null() {
                input.pointer = None;
                return Ok(());
            }
            let xy = arr(channel, value, 2)?;
            if !(xy[0].is_finite() && xy[1].is_finite())
                || !(0.0..=1.0).contains(&xy[0])
                || !(0.0..=1.0).contains(&xy[1])
            {
                return Err(format!(
                    "channel '{channel}' expects normalized coordinates in [0,1], got [{}, {}]",
                    xy[0], xy[1]
                ));
            }
            let pointer = input
                .pointer
                .get_or_insert(shock2vr::input_context::Pointer2D {
                    position: cgmath::vec2(0.0, 0.0),
                    pressed: false,
                });
            pointer.position = cgmath::vec2(xy[0], xy[1]);
            Ok(())
        }
        "pointer.pressed" => {
            let pressed = match num(channel, value)? {
                v if v == 0.0 => false,
                v if v == 1.0 => true,
                v => {
                    return Err(format!("channel '{channel}' expects 0 or 1, got {v}"));
                }
            };
            let pointer = input
                .pointer
                .get_or_insert(shock2vr::input_context::Pointer2D {
                    position: cgmath::vec2(0.0, 0.0),
                    pressed: false,
                });
            pointer.pressed = pressed;
            Ok(())
        }
        // Crouch request: like the desktop LeftControl hold. The ACTUAL state
        // can lag (standing up is refused without headroom); observe it via
        // the player's body y (the collider center drops when crouched).
        "crouch" => {
            input.crouch = match num(channel, value)? {
                v if v == 0.0 => false,
                v if v == 1.0 => true,
                v => {
                    return Err(format!("channel '{channel}' expects 0 or 1, got {v}"));
                }
            };
            Ok(())
        }
        _ => Err(format!(
            "unknown input channel '{channel}'; {}",
            input_channels_help()
        )),
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
    // TODO: Track frame counter

    FrameSnapshot {
        frame_index: frame_counter,
        time: TimeInfo {
            elapsed_ms: time.elapsed.as_millis() as f32,
            total_ms: time.total.as_millis() as f32,
        },
        mission: game.scene_name().to_string(),
        player: {
            // Real player state from the active world (position, look, and the
            // held/wielded entities), or zeros when the scene has no player.
            let state = game.player_state();
            PlayerInfo {
                entity_id: state.as_ref().map(|s| s.entity_id),
                position: state
                    .as_ref()
                    .map(|s| s.position)
                    .unwrap_or([0.0, 0.0, 0.0]),
                rotation: state
                    .as_ref()
                    .map(|s| s.rotation)
                    .unwrap_or([1.0, 0.0, 0.0, 0.0]),
                camera_offset: [0.0, shock2vr::PLAYER_EYE_HEIGHT / SCALE_FACTOR, 0.0],
                camera_rotation: [1.0, 0.0, 0.0, 0.0], // TODO: Get camera rotation
                wielded_entity_id: state.as_ref().and_then(|s| s.wielded_entity_id),
                right_hand_entity_id: state.as_ref().and_then(|s| s.right_hand_entity_id),
                reloading: state.as_ref().is_some_and(|s| s.reloading),
                reload_pitch_deg: state.as_ref().map(|s| s.reload_pitch_deg).unwrap_or(0.0),
                reload_progress: state.as_ref().map(|s| s.reload_progress).unwrap_or(0.0),
                wielded_ammo_type: state.as_ref().and_then(|s| s.wielded_ammo_type.clone()),
                hit_points: state
                    .as_ref()
                    .and_then(|s| s.hit_points.map(|(cur, _)| cur)),
                max_hit_points: state
                    .as_ref()
                    .and_then(|s| s.hit_points.map(|(_, max)| max)),
                psi_points: state
                    .as_ref()
                    .and_then(|s| s.psi_points.map(|(cur, _)| cur)),
                max_psi_points: state
                    .as_ref()
                    .and_then(|s| s.psi_points.map(|(_, max)| max)),
                selected_psi_power: state.as_ref().and_then(|s| s.selected_psi_power.clone()),
                psi_charge: state.as_ref().and_then(|s| s.psi_charge.map(|(f, _)| f)),
                psi_charge_phase: state
                    .as_ref()
                    .and_then(|s| s.psi_charge.map(|(_, p)| p.to_string())),
                active_psi_powers: state
                    .as_ref()
                    .map(|s| s.active_psi_powers.clone())
                    .unwrap_or_default(),
                stats: state.as_ref().and_then(|s| s.stats.clone()),
                collected_logs: state
                    .as_ref()
                    .map(|s| s.collected_logs.clone())
                    .unwrap_or_default(),
                explored_map_locations: state
                    .as_ref()
                    .map(|s| s.explored_map_locations.clone())
                    .unwrap_or_default(),
            }
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
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "instance_id": INSTANCE_ID.get().cloned().flatten(),
    }))
}

/// Get current game state snapshot
async fn get_info(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<FrameSnapshot>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::GetInfo(reply_tx)) {
        tracing::error!("Failed to send GetInfo command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    // Wait for response
    match reply_rx.await {
        Ok(snapshot) => Ok(Json(snapshot)),
        Err(_) => {
            tracing::error!("Failed to receive frame snapshot - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Error response used when the game loop's command channel is gone.
///
/// The HTTP server runs on its own thread, so it can outlive the game
/// thread (e.g. a panic during mission load). Surfacing 503 here keeps
/// clients from mistaking a dead game for a healthy one.
fn game_loop_unavailable() -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Game loop unavailable - the game thread may have crashed (check the debug_runtime process output)".to_string(),
    )
}

/// Step the simulation forward by one frame or time duration
async fn step_frame(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(step_spec): LenientJson<StepSpec>,
) -> Result<Json<StepResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::Step(step_spec, reply_tx)) {
        tracing::error!("Failed to send Step command - game loop receiver dropped");
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Game loop command channel closed - the game thread may have stopped (check the debug_runtime process output)".to_string(),
        ));
    }

    // Wait for response
    match reply_rx.await {
        Ok(Ok(result)) => Ok(Json(result)),
        Ok(Err(StepError::AlreadyInProgress)) => {
            Err((StatusCode::CONFLICT, "Step already in progress".to_string()))
        }
        Err(_) => {
            tracing::error!("Game loop dropped Step reply sender before completion");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Game loop dropped the Step reply before completion (check the debug_runtime process output)".to_string(),
            ))
        }
    }
}

/// Shutdown the debug runtime gracefully
async fn shutdown_server(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    // When launched with --instance-id (SDK-owned), only the owner may shut
    // us down: a client on another checkout whose own runtime failed to bind
    // this port still POSTs /v1/shutdown here during its cleanup, and
    // without this check it kills OUR game mid-test.
    if let Some(Some(expected)) = INSTANCE_ID.get() {
        let provided = body
            .as_ref()
            .and_then(|json| json.0.get("instance_id"))
            .and_then(|v| v.as_str());
        if provided != Some(expected.as_str()) {
            tracing::warn!(
                "Rejected shutdown request with missing/mismatched instance_id (expected {})",
                expected
            );
            return Json(json!({
                "status": "rejected",
                "message": "This runtime is owned by another client; shutdown requires its instance_id",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }));
        }
    }

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

/// Query parameters for entity listing
#[derive(Deserialize)]
struct EntityQueryParams {
    limit: Option<usize>,
    filter: Option<String>,
}

/// List entities with optional filtering and limiting
async fn list_entities(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Query(params): Query<EntityQueryParams>,
) -> Json<EntityListResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::ListEntities {
        limit: params.limit,
        filter: params.filter,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send ListEntities command - game loop receiver dropped");
        return Json(EntityListResult {
            entities: vec![],
            total_count: 0,
            player_position: [0.0, 0.0, 0.0],
        });
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive entity list - sender dropped");
            Json(EntityListResult {
                entities: vec![],
                total_count: 0,
                player_position: [0.0, 0.0, 0.0],
            })
        }
    }
}

/// Get detailed information about a specific entity
async fn get_entity_detail(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(id): Path<i32>,
) -> Json<Option<EntityDetailResult>> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::EntityDetail {
        id,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send EntityDetail command - game loop receiver dropped");
        return Json(None);
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive entity detail - sender dropped");
            Json(None)
        }
    }
}

/// Get animation playback state + world-space posed skeleton for an entity
async fn get_animation_state(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(id): Path<i32>,
) -> Json<Option<shock2vr::game_scene::DebugAnimationState>> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(_) = command_tx.send(RuntimeCommand::AnimationState {
        id,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send AnimationState command - game loop receiver dropped");
        return Json(None);
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive animation state - sender dropped");
            Json(None)
        }
    }
}

/// Inject a script message (damage, frob, signal) into a specific entity.
///
/// Body is a tagged `DebugEntityMessage`, e.g. `{"type":"Damage","amount":1.0}`.
/// Request body for POST /v1/physics/bodies/:id/impulse.
#[derive(serde::Deserialize)]
struct BodyImpulseRequest {
    /// World-space impulse vector (mass * velocity change).
    impulse: [f32; 3],
}

/// HTTP handler: apply a world-space impulse to a dynamic physics body,
/// waking it if asleep. Lets tests poke a settled/sleeping ragdoll headlessly.
async fn apply_body_impulse(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(id): Path<u32>,
    LenientJson(request): LenientJson<BodyImpulseRequest>,
) -> Json<CommandResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(_) = command_tx.send(RuntimeCommand::ApplyBodyImpulse {
        body_id: id,
        impulse: request.impulse,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send ApplyBodyImpulse command - game loop receiver dropped");
        return Json(CommandResult {
            success: false,
            message: "Game loop unavailable".to_string(),
            data: None,
        });
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive impulse result - sender dropped");
            Json(CommandResult {
                success: false,
                message: "No response from game loop".to_string(),
                data: None,
            })
        }
    }
}

async fn send_entity_message(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(id): Path<i32>,
    LenientJson(message): LenientJson<shock2vr::game_scene::DebugEntityMessage>,
) -> Json<CommandResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(_) = command_tx.send(RuntimeCommand::SendEntityMessage {
        id,
        message,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send SendEntityMessage command - game loop receiver dropped");
        return Json(CommandResult {
            success: false,
            message: "Game loop unavailable".to_string(),
            data: None,
        });
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive send-message result - sender dropped");
            Json(CommandResult {
                success: false,
                message: "No response from game loop".to_string(),
                data: None,
            })
        }
    }
}

/// Request structure for player teleportation
#[derive(serde::Deserialize)]
struct TeleportRequest {
    x: f32,
    y: f32,
    z: f32,
}

/// Response structure for player teleportation
#[derive(serde::Serialize)]
struct TeleportResponse {
    success: bool,
    message: String,
    new_position: [f32; 3],
}

/// Response structure for player position
#[derive(serde::Serialize)]
struct PositionResponse {
    position: [f32; 3],
}

/// Request payload for executing a gameplay command
#[derive(serde::Deserialize)]
struct GameCommandRequest {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

/// Request payload for pathfinding test command
#[derive(serde::Deserialize)]
struct PathfindingTestRequest {
    action: String, // "set_start", "set_goal", or "reset"
}

/// HTTP handler for getting player position
async fn get_player_position(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<PositionResponse>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::GetPlayerPosition(reply_tx)) {
        tracing::error!("Failed to send GetPlayerPosition command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    // Wait for response
    match reply_rx.await {
        Ok(position) => Ok(Json(PositionResponse {
            position: [position.x, position.y, position.z],
        })),
        Err(_) => {
            tracing::error!("Failed to receive player position - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler for teleporting player
async fn teleport_player(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<TeleportRequest>,
) -> Result<Json<TeleportResponse>, (StatusCode, String)> {
    let target_position = Vector3::new(request.x, request.y, request.z);

    // Send teleport command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::MovePlayer(target_position)) {
        tracing::error!("Failed to send MovePlayer command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    // Get the new position to confirm the teleport
    let (reply_tx, reply_rx) = oneshot::channel();
    if let Err(_) = command_tx.send(RuntimeCommand::GetPlayerPosition(reply_tx)) {
        tracing::error!("Failed to send GetPlayerPosition command after teleport");
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(position) => Ok(Json(TeleportResponse {
            success: true,
            message: "Player teleported successfully".to_string(),
            new_position: [position.x, position.y, position.z],
        })),
        Err(_) => {
            tracing::error!("Failed to receive player position after teleport - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Request structure for a bounded, collision-validated player move.
#[derive(serde::Deserialize)]
struct MoveRequest {
    x: f32,
    y: f32,
    z: f32,
}

/// HTTP handler for a bounded, shape-cast-validated player move. Unlike
/// `/v1/player/teleport`, this clamps the displacement and stops short of any
/// geometry it hits, so the player can never be pushed through a wall or out of
/// bounds.
async fn move_player_validated(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<MoveRequest>,
) -> Result<Json<MoveResult>, (StatusCode, String)> {
    let target = Vector3::new(request.x, request.y, request.z);

    let (reply_tx, reply_rx) = oneshot::channel();
    if let Err(_) = command_tx.send(RuntimeCommand::MovePlayerValidated {
        target,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send MovePlayerValidated command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive validated-move result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Request structure for a level transition (warp)
#[derive(serde::Deserialize)]
struct TransitionLevelRequest {
    /// Target mission - with or without the ".mis" suffix (e.g. "eng1" or "eng1.mis").
    level: String,
    /// Optional spawn-marker id (`PropStartLoc`); omit for the map default spawn.
    #[serde(default)]
    loc: Option<i32>,
}

/// HTTP handler for transitioning to another level (warp). Lets a tester jump
/// directly to any mission in isolation instead of reaching an in-game trigger.
async fn transition_level(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<TransitionLevelRequest>,
) -> Result<Json<commands::TransitionLevelResult>, (StatusCode, String)> {
    // Normalize to a bare mission filename (e.g. "eng1.mis"), then validate it
    // BEFORE dispatching. The load path does `File::open(...).unwrap()`, so a
    // missing/typo'd name would panic the game-loop thread and brick the runtime
    // for every later command - the opposite of a resilient tester lever. (A
    // structurally-corrupt but present mission - e.g. the known shodan.mis load
    // crash, #267 - can still panic during parse; that is a pre-existing engine
    // limitation, not introduced here.)
    let level = request.level.trim();
    if level.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "level must not be empty".to_string(),
        ));
    }
    if level.contains('/') || level.contains('\\') || level.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("level must be a bare mission name, got '{}'", level),
        ));
    }
    let level_file = if level.to_ascii_lowercase().ends_with(".mis") {
        level.to_string()
    } else {
        format!("{}.mis", level)
    };
    let resolved = shock2vr::resource_path(&level_file);
    if !std::path::Path::new(&resolved).exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "mission '{}' not found (looked at {})",
                level_file, resolved
            ),
        ));
    }

    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::TransitionLevel {
            level_file,
            loc: request.loc,
            reply: reply_tx,
        })
        .is_err()
    {
        tracing::error!("Failed to send TransitionLevel command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive transition result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Request body for a save or load request. `file` is a bare save name.
#[derive(serde::Deserialize)]
struct SaveLoadRequest {
    /// Bare save name (no extension / path separators), e.g. "frontier".
    file: String,
}

/// Validate a bare save name before it reaches the game loop / filesystem.
///
/// The save/load path builds an on-disk path from this name, so a name with a
/// path separator or `..` could escape the saves directory. Reject those (and
/// empties) with a 400 up front rather than trusting the caller.
fn validate_save_name(file: &str) -> Result<String, (StatusCode, String)> {
    let file = file.trim();
    if file.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "file must not be empty".to_string(),
        ));
    }
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("file must be a bare save name, got '{}'", file),
        ));
    }
    Ok(file.to_string())
}

/// HTTP handler for saving the current game to a named file. Persists a
/// "frontier" save that a later runtime launch can reload to resume - the core
/// of the automated play-through loop's cross-launch resume.
async fn save_game(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<SaveLoadRequest>,
) -> Result<Json<commands::SaveLoadResult>, (StatusCode, String)> {
    let file = validate_save_name(&request.file)?;

    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::SaveGame {
            file,
            reply: reply_tx,
        })
        .is_err()
    {
        tracing::error!("Failed to send SaveGame command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(result) if result.success => Ok(Json(result)),
        // The save path unwrapped (I/O error, or a scene without player state);
        // the game loop caught it and stayed alive, so surface a 500 here.
        Ok(result) => Err((StatusCode::INTERNAL_SERVER_ERROR, result.message)),
        Err(_) => {
            tracing::error!("Failed to receive save result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler for loading a named save, restoring the active mission, player
/// position/rotation, quest bits, and held items. Works cross-launch (a fresh
/// runtime started on any mission can load a frontier save and resume).
async fn load_game(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<SaveLoadRequest>,
) -> Result<Json<commands::SaveLoadResult>, (StatusCode, String)> {
    let file = validate_save_name(&request.file)?;

    // The load path does `File::open(...).unwrap()`, so a missing save would
    // panic the game-loop thread and brick the runtime for every later command.
    // Reject a nonexistent save up front with 404 instead.
    let resolved = shock2vr::save_file_path(&file);
    if !resolved.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "save '{}' not found (looked at {})",
                file,
                resolved.display()
            ),
        ));
    }

    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::LoadGame {
            file,
            reply: reply_tx,
        })
        .is_err()
    {
        tracing::error!("Failed to send LoadGame command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(result) if result.success => Ok(Json(result)),
        // The save existed but was corrupt / schema-incompatible: the game loop
        // caught the panic and kept the previous scene, so surface a 500 rather
        // than a misleading success.
        Ok(result) => Err((StatusCode::INTERNAL_SERVER_ERROR, result.message)),
        Err(_) => {
            tracing::error!("Failed to receive load result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Request body for setting a quest bit.
#[derive(serde::Deserialize)]
struct SetQuestBitRequest {
    /// "unknown", "incomplete", or "complete".
    value: String,
}

/// HTTP handler for snapshotting quest bits (objective flags). An empty list
/// means the level has set no quest bits yet (all objectives read as unknown).
async fn get_ui_state(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<commands::UiStateResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::GetUiState { reply: reply_tx })
        .is_err()
    {
        tracing::error!("Failed to send GetUiState command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => Err(game_loop_unavailable()),
    }
}

async fn get_quest_bits(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<commands::QuestBitsResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::GetQuestBits { reply: reply_tx })
        .is_err()
    {
        tracing::error!("Failed to send GetQuestBits command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive quest bits - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler for setting a quest bit (test setup / skipping ahead).
async fn set_quest_bit(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(name): Path<String>,
    LenientJson(request): LenientJson<SetQuestBitRequest>,
) -> Result<Json<commands::CommandResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::SetQuestBit {
            name: name.clone(),
            value: request.value.clone(),
            reply: reply_tx,
        })
        .is_err()
    {
        tracing::error!("Failed to send SetQuestBit command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(Ok(())) => Ok(Json(commands::CommandResult {
            success: true,
            message: format!("Set quest bit '{}' to '{}'", name, request.value),
            data: None,
        })),
        Ok(Err(e)) => Err((StatusCode::BAD_REQUEST, e)),
        Err(_) => {
            tracing::error!("Failed to receive set-quest-bit result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// Request body for giving an item to the player.
#[derive(serde::Deserialize)]
struct GiveItemRequest {
    /// Runtime entity id of an existing world item (as listed by /v1/entities).
    entity_id: i32,
}

/// HTTP handler for putting an existing world entity into the player's
/// inventory - a headless "pick up" for tests.
async fn give_item(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<GiveItemRequest>,
) -> Result<Json<commands::CommandResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::GiveItem {
            entity_id: request.entity_id,
            reply: reply_tx,
        })
        .is_err()
    {
        tracing::error!("Failed to send GiveItem command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(Ok(())) => Ok(Json(commands::CommandResult {
            success: true,
            message: format!("Gave entity {} to the player", request.entity_id),
            data: None,
        })),
        Ok(Err(e)) => Err((StatusCode::BAD_REQUEST, e)),
        Err(_) => {
            tracing::error!("Failed to receive give-item result - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler for snapshotting the player's carried inventory. An empty list
/// means the player is carrying nothing (or the scene has no player).
async fn get_player_inventory(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<commands::PlayerInventoryResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::GetPlayerInventory { reply: reply_tx })
        .is_err()
    {
        tracing::error!("Failed to send GetPlayerInventory command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive player inventory - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler listing level-transition triggers (dest + position), so a tester
/// can follow the real triggers between levels instead of warping explicitly.
async fn list_transitions(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<commands::TransitionsResult>, (StatusCode, String)> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if command_tx
        .send(RuntimeCommand::ListTransitions { reply: reply_tx })
        .is_err()
    {
        tracing::error!("Failed to send ListTransitions command - game loop receiver dropped");
        return Err(game_loop_unavailable());
    }
    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive transitions - sender dropped");
            Err(game_loop_unavailable())
        }
    }
}

/// HTTP handler for physics raycast
async fn perform_raycast(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<RayCastRequest>,
) -> Json<RayCastResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send raycast command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::RayCast(request, reply_tx)) {
        tracing::error!("Failed to send RayCast command - game loop receiver dropped");
        return Json(RayCastResult {
            hit: false,
            hit_point: None,
            hit_normal: None,
            distance: None,
            entity_id: None,
            entity_name: None,
            collision_group: None,
            is_sensor: false,
        });
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive raycast result - sender dropped");
            Json(RayCastResult {
                hit: false,
                hit_point: None,
                hit_normal: None,
                distance: None,
                entity_id: None,
                entity_name: None,
                collision_group: None,
                is_sensor: false,
            })
        }
    }
}

/// Query parameters for physics body listing
#[derive(Deserialize)]
struct PhysicsBodyQueryParams {
    limit: Option<usize>,
    /// Only return bodies whose owning entity id matches (scopes to one ragdoll).
    entity_id: Option<i32>,
}

/// HTTP handler for listing physics bodies
async fn list_physics_bodies(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Query(params): Query<PhysicsBodyQueryParams>,
) -> Json<PhysicsBodyListResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::ListPhysicsBodies {
        limit: params.limit,
        entity_id: params.entity_id,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send ListPhysicsBodies command - game loop receiver dropped");
        return Json(PhysicsBodyListResult {
            bodies: vec![],
            total_count: 0,
            player_position: [0.0, 0.0, 0.0],
        });
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive physics body list - sender dropped");
            Json(PhysicsBodyListResult {
                bodies: vec![],
                total_count: 0,
                player_position: [0.0, 0.0, 0.0],
            })
        }
    }
}

/// HTTP handler for getting detailed physics body information
async fn get_physics_body_detail(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    Path(id): Path<u32>,
) -> Json<Option<PhysicsBodyDetailResult>> {
    let (reply_tx, reply_rx) = oneshot::channel();

    // Send command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::PhysicsBodyDetail {
        id,
        reply: reply_tx,
    }) {
        tracing::error!("Failed to send PhysicsBodyDetail command - game loop receiver dropped");
        return Json(None);
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive physics body detail - sender dropped");
            Json(None)
        }
    }
}

/// HTTP handler for per-ragdoll quality metrics
async fn get_ragdoll_metrics(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Json<RagdollMetricsResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(_) = command_tx.send(RuntimeCommand::RagdollMetrics { reply: reply_tx }) {
        tracing::error!("Failed to send RagdollMetrics command - game loop receiver dropped");
        return Json(RagdollMetricsResult { ragdolls: vec![] });
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive ragdoll metrics - sender dropped");
            Json(RagdollMetricsResult { ragdolls: vec![] })
        }
    }
}

/// HTTP handler for impulse-joint diagnostics (ragdoll constraint health).
async fn list_physics_joints(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Json<commands::PhysicsJointsResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(_) = command_tx.send(RuntimeCommand::ListPhysicsJoints { reply: reply_tx }) {
        tracing::error!("Failed to send ListPhysicsJoints command - game loop receiver dropped");
        return Json(commands::PhysicsJointsResult { joints: vec![] });
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive physics joints - sender dropped");
            Json(commands::PhysicsJointsResult { joints: vec![] })
        }
    }
}

/// HTTP handler for the collider-health audit: reports colliders with
/// malformed world AABBs (NaN/inf, degenerate, or extreme). An empty `issues`
/// list means the level's collider geometry is clean.
async fn audit_colliders(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Json<commands::ColliderAuditResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    let empty = || commands::ColliderAuditResult {
        total_count: 0,
        issues: vec![],
    };
    if command_tx
        .send(RuntimeCommand::AuditColliders { reply: reply_tx })
        .is_err()
    {
        tracing::error!("Failed to send AuditColliders command - game loop receiver dropped");
        return Json(empty());
    }

    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive collider audit - sender dropped");
            Json(empty())
        }
    }
}

/// Request structure for screenshot capture
#[derive(serde::Deserialize)]
struct ScreenshotRequest {
    filename: Option<String>,
}

/// HTTP handler for taking screenshots
async fn take_screenshot(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<ScreenshotRequest>,
) -> Json<ScreenshotResult> {
    let (reply_tx, reply_rx) = oneshot::channel();

    let spec = ScreenshotSpec {
        filename: request.filename,
    };

    // Send screenshot command to game loop
    if let Err(_) = command_tx.send(RuntimeCommand::Screenshot(spec, reply_tx)) {
        tracing::error!("Failed to send Screenshot command - game loop receiver dropped");
        return Json(ScreenshotResult {
            filename: "error.png".to_string(),
            full_path: "/tmp/error.png".to_string(),
            resolution: [0, 0],
            size_bytes: 0,
        });
    }

    // Wait for response
    match reply_rx.await {
        Ok(result) => Json(result),
        Err(_) => {
            tracing::error!("Failed to receive screenshot result - sender dropped");
            Json(ScreenshotResult {
                filename: "error.png".to_string(),
                full_path: "/tmp/error.png".to_string(),
                resolution: [0, 0],
                size_bytes: 0,
            })
        }
    }
}

/// Resolve a screenshot spec to a file path, capture the freshly-rendered frame,
/// and build the result. Called from the game loop after `finish_render` (before
/// the buffer swap) so it reads a complete frame.
fn capture_screenshot_to_result(spec: ScreenshotSpec) -> ScreenshotResult {
    let filename = spec.filename.unwrap_or_else(|| {
        format!(
            "screenshot_{}.png",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        )
    });

    let screenshots_dir = std::path::Path::new("/tmp/claude");
    std::fs::create_dir_all(screenshots_dir).unwrap_or_else(|e| {
        tracing::warn!("Failed to create screenshots directory: {}", e);
    });
    let full_path = screenshots_dir.join(&filename);

    match capture_screenshot(&full_path, SCR_WIDTH, SCR_HEIGHT) {
        Ok(size_bytes) => {
            tracing::info!("Screenshot saved to: {}", full_path.display());
            ScreenshotResult {
                filename,
                full_path: full_path.to_string_lossy().to_string(),
                resolution: [SCR_WIDTH, SCR_HEIGHT],
                size_bytes,
            }
        }
        Err(e) => {
            tracing::error!("Failed to capture screenshot: {}", e);
            ScreenshotResult {
                filename,
                full_path: full_path.to_string_lossy().to_string(),
                resolution: [0, 0],
                size_bytes: 0,
            }
        }
    }
}

/// Capture the current OpenGL framebuffer and save it as a PNG
fn capture_screenshot(
    path: &std::path::Path,
    width: u32,
    height: u32,
) -> Result<u64, Box<dyn std::error::Error>> {
    unsafe {
        // Ensure all rendering for this frame has completed before reading back,
        // so we never capture a partially-drawn buffer.
        gl::Finish();

        // Query the current viewport to see what size it actually is
        let mut viewport: [i32; 4] = [0; 4];
        gl::GetIntegerv(gl::VIEWPORT, viewport.as_mut_ptr());
        tracing::info!(
            "Current viewport: x={}, y={}, width={}, height={}",
            viewport[0],
            viewport[1],
            viewport[2],
            viewport[3]
        );

        // Use actual viewport size for screenshot, falling back to requested size if needed
        let actual_width = if viewport[2] > 0 {
            viewport[2] as u32
        } else {
            tracing::warn!(
                "Viewport width unavailable, using requested width {}",
                width
            );
            width
        };
        let actual_height = if viewport[3] > 0 {
            viewport[3] as u32
        } else {
            tracing::warn!(
                "Viewport height unavailable, using requested height {}",
                height
            );
            height
        };

        // Read pixels from the framebuffer using actual viewport size
        let mut pixels: Vec<u8> = vec![0; (actual_width * actual_height * 3) as usize];
        gl::ReadPixels(
            0,
            0,
            actual_width as i32,
            actual_height as i32,
            gl::RGB,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr() as *mut gl::types::GLvoid,
        );

        // Check for OpenGL errors
        let error = gl::GetError();
        if error != gl::NO_ERROR {
            return Err(format!("OpenGL error during ReadPixels: {}", error).into());
        }

        // Flip the image vertically (OpenGL origin is bottom-left, PNG is top-left)
        let mut flipped_pixels: Vec<u8> = vec![0; pixels.len()];
        for y in 0..actual_height {
            let src_row = y as usize * actual_width as usize * 3;
            let dst_row = (actual_height - 1 - y) as usize * actual_width as usize * 3;
            flipped_pixels[dst_row..dst_row + actual_width as usize * 3]
                .copy_from_slice(&pixels[src_row..src_row + actual_width as usize * 3]);
        }

        // Create image and save as PNG using actual dimensions
        let img = image::RgbImage::from_vec(actual_width, actual_height, flipped_pixels)
            .ok_or("Failed to create image from pixel data")?;

        img.save(path)?;

        // Calculate file size
        let metadata = std::fs::metadata(path)?;
        Ok(metadata.len())
    }
}

/// HTTP endpoint handler: Get current input state
async fn get_input_state(
    State(command_tx): State<tokio::sync::mpsc::UnboundedSender<commands::RuntimeCommand>>,
) -> Result<Json<commands::InputState>, StatusCode> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    if command_tx
        .send(commands::RuntimeCommand::GetInput(reply_tx))
        .is_err()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(input_state) => Ok(Json(input_state)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Parse a `/v1/control/input` POST body into input patches, accepting either
/// shape:
/// - explicit:  `{"channel": "right_hand.trigger", "value": 1.0}`
/// - map form:  `{"right_hand.trigger": 1.0, "head.look": [30, 0]}`
///
/// The map form is detected when the object does NOT have both `channel` and
/// `value` keys. Returns an actionable error (not a silent empty patch) when the
/// body isn't a usable object, so a malformed request fails loudly.
fn parse_input_patches(body: &serde_json::Value) -> Result<Vec<commands::InputPatch>, String> {
    let obj = body.as_object().ok_or_else(|| {
        format!(
            "request body must be a JSON object, e.g. {{\"channel\":\"right_hand.trigger\",\"value\":1.0}} \
             or {{\"right_hand.trigger\":1.0}}; {}",
            input_channels_help()
        )
    })?;

    // Explicit {channel, value} form.
    if obj.contains_key("channel") && obj.contains_key("value") {
        let channel = obj["channel"]
            .as_str()
            .ok_or_else(|| "\"channel\" must be a string".to_string())?
            .to_string();
        return Ok(vec![commands::InputPatch {
            channel,
            value: obj["value"].clone(),
        }]);
    }

    if obj.is_empty() {
        return Err(format!(
            "no input channels in request; {}",
            input_channels_help()
        ));
    }

    // Map form: every key is a channel name.
    Ok(obj
        .iter()
        .map(|(channel, value)| commands::InputPatch {
            channel: channel.clone(),
            value: value.clone(),
        })
        .collect())
}

/// HTTP endpoint handler: Set one or more input channel values.
async fn set_input_channel(
    State(command_tx): State<tokio::sync::mpsc::UnboundedSender<commands::RuntimeCommand>>,
    LenientJson(body): LenientJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let patches = parse_input_patches(&body).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if command_tx
        .send(commands::RuntimeCommand::SetInput(patches, reply_tx))
        .is_err()
    {
        return Err(game_loop_unavailable());
    }

    match reply_rx.await {
        Ok(Ok(())) => Ok(Json(serde_json::json!({
            "success": true,
            "message": "Input channel(s) updated"
        }))),
        // An invalid channel/value: surface it as an actionable 400 instead of
        // the previous silent success.
        Ok(Err(msg)) => Err((StatusCode::BAD_REQUEST, msg)),
        Err(_) => Err(game_loop_unavailable()),
    }
}

/// HTTP endpoint handler: Execute a gameplay command via the runtime
async fn run_game_command(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<GameCommandRequest>,
) -> Result<Json<CommandResult>, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::RunGameCommand(
            request.command,
            request.args,
            reply_tx,
        ))
        .is_err()
    {
        tracing::error!("Failed to send RunGameCommand - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive RunGameCommand result - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// HTTP endpoint handler: Execute a pathfinding test command
async fn pathfinding_test(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<PathfindingTestRequest>,
) -> Result<Json<CommandResult>, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::PathfindingTest(request.action, reply_tx))
        .is_err()
    {
        tracing::error!("Failed to send PathfindingTest command - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive PathfindingTest result - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// HTTP endpoint handler: Get the current pathfinding test status
async fn pathfinding_test_status(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<PathfindingTestStatusResult>, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::GetPathfindingTestStatus(reply_tx))
        .is_err()
    {
        tracing::error!("Failed to send GetPathfindingTestStatus - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive pathfinding test status - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// HTTP endpoint handler: Get the pathfinding service's query counters.
/// Returns JSON `null` when the scene has no pathfinding data.
async fn pathfinding_stats(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<Option<shock2vr::game_scene::DebugPathfindingStats>>, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::GetPathfindingStats(reply_tx))
        .is_err()
    {
        tracing::error!("Failed to send GetPathfindingStats - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive pathfinding stats - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// HTTP endpoint handler: the latest path each AI computed (goal, waypoints,
/// outcome: Full/Partial/Failed). Empty until an AI has pathed.
async fn ai_paths(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
) -> Result<Json<Vec<shock2vr::game_scene::DebugAiPathEntry>>, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::GetAiPaths(reply_tx))
        .is_err()
    {
        tracing::error!("Failed to send GetAiPaths - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive AI paths - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
struct TriggerActionRequest {
    action: String,
}

/// HTTP endpoint handler: Trigger a discrete input action
async fn trigger_input_action(
    State(command_tx): State<mpsc::UnboundedSender<RuntimeCommand>>,
    LenientJson(request): LenientJson<TriggerActionRequest>,
) -> Result<Json<CommandResult>, StatusCode> {
    let action = match request.action.parse::<InputAction>() {
        Ok(action) => action,
        Err(_) => {
            let available: Vec<&str> = InputAction::all().iter().map(|a| a.as_str()).collect();
            return Ok(Json(CommandResult {
                success: false,
                message: format!(
                    "Unknown action '{}'. Available actions: {}",
                    request.action,
                    available.join(", ")
                ),
                data: None,
            }));
        }
    };

    let (reply_tx, reply_rx) = oneshot::channel();

    if command_tx
        .send(RuntimeCommand::TriggerAction(action, reply_tx))
        .is_err()
    {
        tracing::error!("Failed to send TriggerAction command - game loop receiver dropped");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    match reply_rx.await {
        Ok(result) => Ok(Json(result)),
        Err(_) => {
            tracing::error!("Failed to receive TriggerAction result - sender dropped");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// HTTP endpoint handler: List available input actions
async fn list_input_actions() -> Json<Value> {
    let actions: Vec<&str> = InputAction::all().iter().map(|a| a.as_str()).collect();
    Json(serde_json::json!({ "actions": actions }))
}

/// HTTP handler for the recently played environmental sounds (resolved schema
/// sample + query tags + position). This is the only headless way to observe
/// audio, e.g. asserting a bullet impact played a material-tagged collision
/// schema. Reads a process-wide log, so no game-loop round-trip is needed.
async fn get_recent_audio() -> Json<Value> {
    Json(serde_json::json!({ "sounds": shock2vr::audio_log::recent() }))
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
