// Command processing for debug runtime
//
// This module defines the command interface between the HTTP server and game loop,
// allowing remote control of the running game through a request/response pattern.

use cgmath::Vector3;
use serde::{Deserialize, Serialize};
use shock2vr::input::InputAction;
use tokio::sync::oneshot;

/// Commands that can be sent from HTTP handlers to the game loop
#[derive(Debug)]
pub enum RuntimeCommand {
    /// Get current game state snapshot
    GetInfo(oneshot::Sender<FrameSnapshot>),

    /// Step the simulation forward by frames or time
    Step(StepSpec, oneshot::Sender<Result<StepResult, StepError>>),

    /// Take a screenshot of the current frame
    Screenshot(ScreenshotSpec, oneshot::Sender<ScreenshotResult>),

    /// Perform a physics raycast
    RayCast(RayCastRequest, oneshot::Sender<RayCastResult>),

    /// Get current input state
    GetInput(oneshot::Sender<InputState>),

    /// Set one or more input channel values. Replies `Ok(())` when every patch
    /// applied, or `Err(message)` describing the first invalid channel/value so
    /// the HTTP layer can return an actionable 400 instead of a silent success.
    SetInput(Vec<InputPatch>, oneshot::Sender<Result<(), String>>),

    /// Move the player to a position
    MovePlayer(Vector3<f32>),

    /// Move the player toward a target with a bounded, shape-cast-validated hop
    /// (won't pass through walls / out of bounds). See `MoveResult`.
    MovePlayerValidated {
        target: Vector3<f32>,
        reply: oneshot::Sender<MoveResult>,
    },

    /// Transition to another level (warp), at an optional spawn-marker id.
    TransitionLevel {
        level_file: String,
        loc: Option<i32>,
        reply: oneshot::Sender<TransitionLevelResult>,
    },

    /// Save the current game to a named save file (frontier persistence).
    SaveGame {
        file: String,
        reply: oneshot::Sender<SaveLoadResult>,
    },

    /// Load a previously-saved game, restoring mission/player/quests/items.
    LoadGame {
        file: String,
        reply: oneshot::Sender<SaveLoadResult>,
    },

    /// Snapshot the flat-mode UI state (mode: shooter/use).
    GetUiState {
        reply: oneshot::Sender<UiStateResult>,
    },

    /// Snapshot the quest bits (objective flags) the game has set.
    GetQuestBits {
        reply: oneshot::Sender<QuestBitsResult>,
    },

    /// Set a quest bit (objective flag) - for test setup / skipping ahead.
    SetQuestBit {
        name: String,
        value: String,
        reply: oneshot::Sender<Result<(), String>>,
    },

    /// Snapshot the player's carried inventory.
    GetPlayerInventory {
        reply: oneshot::Sender<PlayerInventoryResult>,
    },

    /// List level-transition triggers (dest + position) for trigger-based traversal.
    ListTransitions {
        reply: oneshot::Sender<TransitionsResult>,
    },

    /// Put an existing world entity into the player's inventory (headless pickup).
    GiveItem {
        entity_id: i32,
        reply: oneshot::Sender<Result<(), String>>,
    },

    /// Provision a fresh item from a template into the player's inventory.
    SpawnItem {
        template: shock2vr::game_scene::DebugItemTemplate,
        reply: oneshot::Sender<Result<shock2vr::game_scene::DebugSpawnedItem, String>>,
    },

    /// Provision the player's character sheet (stats/skills/psi tier/modules).
    SetPlayerStats {
        request: shock2vr::game_scene::DebugPlayerStatsRequest,
        reply: oneshot::Sender<Result<shock2vr::player_stats::PlayerStats, String>>,
    },

    /// Get current player position
    GetPlayerPosition(oneshot::Sender<Vector3<f32>>),

    /// Get the free (debug) camera's state: detached, and the pose it is
    /// rendering from when it is.
    GetCameraState(oneshot::Sender<CameraStateSnapshot>),

    /// Pathfinding test command (set_start, set_goal, reset)
    PathfindingTest(String, oneshot::Sender<CommandResult>),

    /// Trigger a discrete input action (as if a bound key was pressed)
    TriggerAction(InputAction, oneshot::Sender<CommandResult>),

    /// Get the current pathfinding test status
    GetPathfindingTestStatus(oneshot::Sender<PathfindingTestStatusResult>),

    /// Get the pathfinding service's monotonic query counters (None when the
    /// scene has no pathfinding data)
    GetPathfindingStats(oneshot::Sender<Option<shock2vr::game_scene::DebugPathfindingStats>>),

    /// Get the latest path each AI computed (goal, waypoints, outcome)
    GetAiPaths(oneshot::Sender<Vec<shock2vr::game_scene::DebugAiPathEntry>>),

    /// List entities near the player
    ListEntities {
        limit: Option<usize>,
        filter: Option<String>,
        reply: oneshot::Sender<EntityListResult>,
    },

    /// Get detailed information about an entity
    EntityDetail {
        id: i32,
        reply: oneshot::Sender<Option<EntityDetailResult>>,
    },

    /// Get animation playback state + posed skeleton for an entity
    AnimationState {
        id: i32,
        reply: oneshot::Sender<Option<shock2vr::game_scene::DebugAnimationState>>,
    },

    /// Inject a script message into a specific entity (damage, frob, signal)
    SendEntityMessage {
        id: i32,
        message: shock2vr::game_scene::DebugEntityMessage,
        reply: oneshot::Sender<CommandResult>,
    },

    /// List physics rigid bodies
    ListPhysicsBodies {
        limit: Option<usize>,
        /// Optional filter: only return bodies owned by this entity id. Useful
        /// for scoping to the many bodies of a single ragdoll.
        entity_id: Option<i32>,
        reply: oneshot::Sender<PhysicsBodyListResult>,
    },

    /// Describe the scene objects handed to the renderer on the last drawn
    /// frame - what is actually being rendered, and with what transparency,
    /// depth-write and backface-culling state.
    ListSceneObjects {
        /// Only objects belonging to this entity id.
        entity_id: Option<i32>,
        /// Only objects that are not fully opaque.
        transparent_only: bool,
        limit: Option<usize>,
        reply: oneshot::Sender<SceneListResult>,
    },

    /// Get detailed information about a physics body
    PhysicsBodyDetail {
        id: u32,
        reply: oneshot::Sender<Option<PhysicsBodyDetailResult>>,
    },

    /// Get per-ragdoll quality/settle metrics
    RagdollMetrics {
        reply: oneshot::Sender<RagdollMetricsResult>,
    },

    /// List impulse + multibody joints with anchor separation + applied impulse
    ListPhysicsJoints {
        reply: oneshot::Sender<PhysicsJointsResult>,
    },

    /// Apply a world-space impulse to a dynamic physics body (waking it) -
    /// e.g. poke a settled ragdoll to verify it wakes and reacts.
    ApplyBodyImpulse {
        body_id: u32,
        impulse: [f32; 3],
        reply: oneshot::Sender<CommandResult>,
    },

    /// Audit collider AABBs for malformed geometry (NaN/degenerate/extreme)
    AuditColliders {
        reply: oneshot::Sender<ColliderAuditResult>,
    },

    /// Shutdown the debug runtime gracefully
    Shutdown,
}

/// Specification for stepping the simulation
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StepSpec {
    /// Step by number of frames
    Frames { frames: u32 },
    /// Step by duration
    Duration { duration: String }, // Will be parsed with humantime
}

/// Error returned when a step command cannot be started.
#[derive(Debug)]
pub enum StepError {
    /// Another step request is still advancing the simulation.
    AlreadyInProgress,
}

/// Result of stepping the simulation
#[derive(Debug, Serialize)]
pub struct StepResult {
    pub frames_advanced: u32,
    pub time_advanced: f32,
    pub new_frame_index: u64,
    pub new_total_time: f32,
}

/// Specification for taking a screenshot
#[derive(Debug, Deserialize)]
pub struct ScreenshotSpec {
    pub filename: Option<String>,
}

/// Result of taking a screenshot
#[derive(Debug, Serialize)]
pub struct ScreenshotResult {
    pub filename: String,
    pub full_path: String,
    pub resolution: [u32; 2],
    pub size_bytes: u64,
}

/// Request for physics raycast
#[derive(Debug, Deserialize)]
pub struct RayCastRequest {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub collision_groups: Option<Vec<String>>,
    pub max_distance: Option<f32>,
    /// Defaults to true; pass false to probe trigger and other sensor volumes.
    pub ignore_sensors: Option<bool>,
}

/// Result of physics raycast
#[derive(Debug, Serialize)]
pub struct RayCastResult {
    pub hit: bool,
    pub hit_point: Option<[f32; 3]>,
    pub hit_normal: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub body_id: Option<u32>,
    pub collision_group: Option<String>,
    pub is_sensor: bool,
}

/// Scene objects submitted on the last rendered frame
#[derive(Debug, Serialize)]
pub struct SceneListResult {
    pub objects: Vec<SceneObjectSummary>,
    /// Objects in the frame before any filtering.
    pub total_count: usize,
    /// Objects matching the filters, before `limit`.
    pub matched_count: usize,
    /// Frame index the snapshot came from.
    pub frame_index: u64,
}

/// One scene object as submitted to the renderer
#[derive(Debug, Serialize)]
pub struct SceneObjectSummary {
    pub entity_id: Option<u64>,
    pub name: Option<String>,
    pub model: Option<String>,
    /// Render path that produced it, e.g. "entity". Absent for engine-built
    /// geometry (world, HUD, debug overlays).
    pub source: Option<String>,
    pub position: [f32; 3],
    /// Transparency in effect for this draw (0.0 = opaque, 1.0 = invisible).
    pub transparency: Option<f32>,
    pub depth_write: bool,
    /// Explicit renderer composition layer.
    pub render_layer: String,
    /// Backwards-compatible indication that this object begins a depth-cleared
    /// layer. Depth is cleared once by the renderer, not by the object.
    pub clear_depth: bool,
    /// Front-face winding used for culling, or absent when double-sided.
    pub backface_culling: Option<String>,
}

/// List of physics rigid bodies
#[derive(Debug, Serialize)]
pub struct PhysicsBodyListResult {
    pub bodies: Vec<PhysicsBodySummary>,
    pub total_count: usize,
    pub player_position: [f32; 3],
}

/// Summary information about a physics body
#[derive(Debug, Serialize)]
pub struct PhysicsBodySummary {
    pub body_id: u32,
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub body_type: String, // "dynamic", "static", "kinematic"
    pub position: [f32; 3],
    pub rotation: [f32; 4], // quaternion
    pub mass: Option<f32>,
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub collision_groups: Vec<String>,
    /// Whether this body stops the player capsule (the collider's filter),
    /// which `collision_groups` - membership only - cannot show.
    pub blocks_player: bool,
    /// Whether this body stops a living creature capsule.
    pub blocks_actor: bool,
    pub is_sensor: bool,
    pub is_enabled: bool,
    pub is_sleeping: bool,
}

/// Per-ragdoll quality/settle metrics
#[derive(Debug, Serialize)]
pub struct RagdollMetricsResult {
    pub ragdolls: Vec<RagdollMetricsEntry>,
}

#[derive(Debug, Serialize)]
pub struct RagdollMetricsEntry {
    pub entity_id: i32,
    pub body_count: usize,
    pub max_linear_speed: f32,
    pub max_angular_speed: f32,
    pub min_y: f32,
    pub max_nonadjacent_overlap: f32,
    pub max_drift: f32,
}

/// Joint diagnostics (ragdoll constraint health), impulse + multibody.
#[derive(Debug, Serialize)]
pub struct PhysicsJointsResult {
    pub joints: Vec<PhysicsJointEntry>,
}

#[derive(Debug, Serialize)]
pub struct PhysicsJointEntry {
    pub body1_id: u32,
    pub body2_id: u32,
    /// `"impulse"` or `"multibody"` - which joint set this came from.
    pub joint_type: String,
    pub bone1: Option<u32>,
    pub bone2: Option<u32>,
    pub anchor1: [f32; 3],
    pub anchor2: [f32; 3],
    pub separation: f32,
    pub linear_impulse: f32,
    pub angular_impulse: f32,
}

/// Result of the collider-health audit: colliders with malformed AABBs.
/// `total_count` is the number of issues; an empty `issues` list means the
/// level's collider geometry is clean.
#[derive(Debug, Serialize)]
pub struct ColliderAuditResult {
    pub total_count: usize,
    pub issues: Vec<ColliderIssueEntry>,
}

#[derive(Debug, Serialize)]
pub struct ColliderIssueEntry {
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub kind: String,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
    pub is_sensor: bool,
}

/// Detailed information about a physics body
#[derive(Debug, Serialize)]
pub struct PhysicsBodyDetailResult {
    pub body_id: u32,
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub body_type: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4], // quaternion
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub mass: Option<f32>,
    pub center_of_mass: [f32; 3],
    pub moment_of_inertia: Option<[f32; 3]>,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub collision_groups: Vec<String>,
    /// See `PhysicsBodySummary::blocks_player`.
    pub blocks_player: bool,
    /// See `PhysicsBodySummary::blocks_actor`.
    pub blocks_actor: bool,
    pub is_sensor: bool,
    pub is_enabled: bool,
    pub is_sleeping: bool,
    pub contact_count: usize,
}

/// Input channel modifications
#[derive(Debug, Deserialize)]
pub struct InputPatch {
    pub channel: String,
    pub value: serde_json::Value,
}

/// Complete input state for reading/setting
#[derive(Debug, Serialize, Deserialize)]
pub struct InputState {
    pub head: InputHead,
    pub left_hand: InputHand,
    pub right_hand: InputHand,
    /// 2D screen pointer (flat-mode cursor); `None` until a pointer channel
    /// is set. Position is normalized [0,1] per axis, origin top-left.
    pub pointer: Option<InputPointer>,
    /// Crouch REQUEST (the `crouch` channel), not the resulting collider
    /// state - standing up is refused while there is no headroom.
    pub crouch: bool,
    /// Ordinary held jump request. The physics controller launches only on
    /// its rising edge and only while grounded.
    pub jump: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputPointer {
    pub position: [f32; 2],
    pub pressed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputHead {
    pub rotation: [f32; 4], // Quaternion [x, y, z, w]
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputHand {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion [x, y, z, w]
    pub thumbstick: [f32; 2],
    pub trigger_value: f32,
    pub squeeze_value: f32,
    pub a_value: f32,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            head: InputHead::default(),
            left_hand: InputHand::default(),
            right_hand: InputHand::default(),
            pointer: None,
            crouch: false,
            jump: false,
        }
    }
}

impl Default for InputHead {
    fn default() -> Self {
        Self {
            rotation: [0.0, 0.0, 0.0, 1.0], // Identity quaternion
        }
    }
}

impl Default for InputHand {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0], // Identity quaternion
            thumbstick: [0.0, 0.0],
            trigger_value: 0.0,
            squeeze_value: 0.0,
            a_value: 0.0,
        }
    }
}

/// Result of executing a game command
#[derive(Debug, Serialize)]
pub struct CommandResult {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Result of a bounded, shape-cast-validated player move (`/v1/player/move`).
#[derive(Debug, Serialize)]
pub struct MoveResult {
    /// Whether the player position actually changed.
    pub moved: bool,
    /// Whether the shape cast hit geometry before the full clamped distance.
    pub blocked: bool,
    pub new_position: [f32; 3],
    /// How far the player actually advanced (world units).
    pub distance_moved: f32,
    /// The distance the move was allowed to attempt: `min(target distance, 5.0)`.
    pub requested_distance: f32,
}

/// Result of a save-to-file or load-from-file request.
#[derive(Debug, Serialize)]
pub struct SaveLoadResult {
    pub success: bool,
    /// The bare save name (no extension) that was saved/loaded.
    pub file: String,
    /// The active scene name after the operation (e.g. "medsci1.mis"). After a
    /// load this is the restored mission; after a save it is the scene saved.
    pub mission: String,
    pub message: String,
    /// Stable failure code. Omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Human-readable refusal reason. Omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Exact live pose associated with an unsafe-player-state refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_pose: Option<shock2vr::SavePlayerPose>,
}

/// Result of a level transition (warp) request
#[derive(Debug, Serialize)]
pub struct TransitionLevelResult {
    pub success: bool,
    /// The scene name after the transition (e.g. "eng1.mis"). With the loading
    /// screen enabled this may still be the previous level until updates run.
    pub mission: String,
    pub message: String,
}

/// A single quest bit (objective flag) and its 3-state value.
#[derive(Debug, Serialize)]
pub struct QuestBitEntry {
    pub name: String,
    /// "unknown", "incomplete", or "complete" (a projection of `bits`).
    pub value: String,
    /// The exact raw flag value (scripts compare quest bits by raw value).
    pub bits: u32,
}

/// Snapshot of all quest bits the game has set.
#[derive(Debug, Serialize)]
pub struct QuestBitsResult {
    pub quests: Vec<QuestBitEntry>,
    pub count: usize,
}

/// Flat-mode UI state snapshot (`GET /v1/ui`): "shooter" or "use", plus the
/// open MFD panel and the use-mode inventory strip (entity binding + labeled
/// clickable elements with canvas and normalized-screen rects). See
/// projects/flat-ui.md.
#[derive(Debug, Serialize)]
pub struct UiStateResult {
    pub mode: String,
    pub active_panel: Option<shock2vr::game_scene::DebugUiPanel>,
    /// The top-docked inventory strip (Tab metagame mode); `Some` exactly in
    /// "use" mode.
    pub strip: Option<shock2vr::game_scene::DebugUiPanel>,
    /// The item held on the cursor mid-drag (cursor-is-the-item); `Some`
    /// between a lift and the place/throw that clears it.
    pub cursor: Option<shock2vr::game_scene::DebugUiCursor>,
    /// The AMMOFULL ammo-cycle button (use mode + multi-ammo weapon); `Some`
    /// when shown. Click it to cycle the wielded weapon's ammo type.
    pub ammo_cycle: Option<shock2vr::game_scene::DebugUiElement>,
    /// Where the pointer last landed on the shared canvas (flat: the mouse;
    /// VR: the controller ray on the cyber-interface panel).
    pub pointer: Option<shock2vr::game_scene::DebugUiPointer>,
    /// The VR cyber-interface panel's pose, in pawn space; `Some` exactly
    /// while the interface is up in VR. Aim a controller at a canvas rect by
    /// mapping it through this.
    pub panel_pose: Option<shock2vr::game_scene::DebugUiPanelPose>,
}

/// A single carried item.
#[derive(Debug, Serialize)]
pub struct InventoryItemEntry {
    pub entity_id: i32,
    pub name: Option<String>,
    /// "inventory" (backpack), "left_hand", or "right_hand".
    pub location: String,
}

/// Snapshot of the player's carried inventory.
#[derive(Debug, Serialize)]
pub struct PlayerInventoryResult {
    pub items: Vec<InventoryItemEntry>,
    pub count: usize,
}

/// A level-transition trigger and where it leads.
#[derive(Debug, Serialize)]
pub struct TransitionEntry {
    pub entity_id: i32,
    pub name: Option<String>,
    /// Destination mission (no ".mis" suffix, e.g. "eng1").
    pub dest_level: String,
    pub dest_loc: Option<i32>,
    pub position: [f32; 3],
}

/// All level-transition triggers in the current scene.
#[derive(Debug, Serialize)]
pub struct TransitionsResult {
    pub transitions: Vec<TransitionEntry>,
    pub count: usize,
}

/// Current status of the interactive pathfinding test system
#[derive(Debug, Serialize)]
pub struct PathfindingTestStatusResult {
    /// "WaitingForStart", "WaitingForGoal", or "ShowingPath"
    pub state: String,
    /// Number of waypoints in the computed test path (0 if no path computed)
    pub test_path_waypoints: usize,
}

/// List of entities
#[derive(Debug, Serialize)]
pub struct EntityListResult {
    pub entities: Vec<EntitySummary>,
    pub total_count: usize,
    pub player_position: [f32; 3],
}

/// Summary information about an entity
#[derive(Debug, Serialize)]
pub struct EntitySummary {
    pub id: i32,
    pub name: String,
    pub template_id: i32,
    pub position: [f32; 3],
    pub distance: f32,
    pub script_count: usize,
    pub link_count: usize,
}

/// Detailed information about an entity
#[derive(Debug, Serialize)]
pub struct EntityDetailResult {
    pub entity_id: i32,
    pub name: String,
    pub template_id: i32,
    pub position: [f32; 3],
    pub rotation: [f32; 4], // quaternion
    pub inheritance_chain: Vec<String>,
    pub properties: Vec<PropertyInfo>,
    pub outgoing_links: Vec<LinkInfo>,
    pub incoming_links: Vec<LinkInfo>,
    pub contained_by: Option<i32>,
    pub aim_points: Vec<AimPointInfo>,
}

#[derive(Debug, Serialize)]
pub struct AimPointInfo {
    pub proxy_entity_id: i32,
    pub body_id: u32,
    pub joint_id: u32,
    pub classification: String,
    pub position: [f32; 3],
}

/// Property information
#[derive(Debug, Serialize)]
pub struct PropertyInfo {
    pub name: String,
    pub value: String,
}

/// Link information
#[derive(Debug, Serialize)]
pub struct LinkInfo {
    pub link_type: String,
    /// The target for an outgoing link and the source for an incoming link.
    pub target_id: i32,
    pub target_name: String,
    pub contains_ordinal: Option<u32>,
}

/// The free (debug) camera's state, as `GET /v1/camera` reports it.
///
/// `position`/`rotation` are `None` while the camera is attached: there is no
/// separate camera pose then, only the player's eye, which
/// `GET /v1/player/position` already reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraStateSnapshot {
    /// Whether the camera is detached from the player.
    pub detached: bool,
    /// Whether the free camera is enabled at all (the developer option).
    pub enabled: bool,
    pub position: Option<[f32; 3]>,
    /// Rotation as `[w, x, y, z]`.
    pub rotation: Option<[f32; 4]>,
}

/// Current state of the game
#[derive(Debug, Serialize, Clone)]
pub struct FrameSnapshot {
    pub frame_index: u64,
    pub time: TimeInfo,
    pub mission: String,
    /// True once the retail finale has entered its terminal ending cutscene.
    pub campaign_completed: bool,
    /// True once a scene has asked the runtime to quit (the main menu's Quit).
    /// The debug runtime deliberately stays up - an automation session must not
    /// kill itself - so this is how the quit path is observed headlessly.
    pub quit_requested: bool,
    /// True while the in-game pause menu is up - the simulation is frozen and
    /// `/v1/step` advances only rendering, so automation can tell "paused" from
    /// "stuck".
    pub paused: bool,
    pub player: PlayerInfo,
    pub entity_count: usize,
    pub debug_features: Vec<String>,
    pub inputs: InputSnapshot,
}

/// Time information
#[derive(Debug, Serialize, Clone)]
pub struct TimeInfo {
    pub elapsed_ms: f32,
    pub total_ms: f32,
}

/// Player information
#[derive(Debug, Serialize, Clone)]
pub struct PlayerInfo {
    pub entity_id: Option<i32>,
    pub inventory_entity_id: Option<i32>,
    pub position: [f32; 3],
    pub rotation: [f32; 4], // quaternion
    /// "alive", terminally "dead", or waiting for QBR reconstruction.
    pub life_state: String,
    pub camera_offset: [f32; 3],
    pub camera_rotation: [f32; 4], // quaternion
    /// The wielded/first-person weapon in flatscreen mode (the player's left-hand
    /// slot); `None` when unarmed. See `shock2vr::PlayerStateSnapshot`.
    pub wielded_entity_id: Option<i32>,
    /// The entity held in the right hand (VR), `None` otherwise.
    pub right_hand_entity_id: Option<i32>,
    /// Whether the wielded weapon is mid-reload, plus the current viewmodel tilt
    /// (degrees) and reload progress (0..1). See `shock2vr::PlayerStateSnapshot`.
    pub reloading: bool,
    pub reload_pitch_deg: f32,
    pub reload_progress: f32,
    /// The wielded weapon's selected ammo type (e.g. "std" / "he" / "ap"), or
    /// `null` when unarmed / melee. See `shock2vr::PlayerStateSnapshot`.
    pub wielded_ammo_type: Option<String>,
    /// The player's current / maximum hit points, or `null` when the player
    /// has no health pool. See `shock2vr::PlayerStateSnapshot`.
    pub hit_points: Option<i32>,
    pub max_hit_points: Option<i32>,
    /// The player's current / maximum psi points, or `null` when the player has
    /// no psi pool. See `shock2vr::PlayerStateSnapshot`.
    pub psi_points: Option<i32>,
    pub max_psi_points: Option<i32>,
    /// The gamesys name of the selected psi power (what the psi amp casts),
    /// e.g. "Cryokinesis". Cycle with the `CyclePsiPower` input action.
    pub selected_psi_power: Option<String>,
    /// The psi amp's hold-to-overload meter fill (0..1) and phase
    /// ("charging" / "overloaded" / "burnout"), or `null` when no charge is
    /// in progress. See `shock2vr::PlayerStateSnapshot`.
    pub psi_charge: Option<f32>,
    pub psi_charge_phase: Option<String>,
    /// The gamesys names of the active sustained psi powers (e.g. "Inviso"),
    /// in activation order; empty when none. See `shock2vr::PlayerStateSnapshot`.
    pub active_psi_powers: Vec<String>,
    /// The player's persistent character sheet (primary stats, trained skills,
    /// mastered psi disciplines), accumulated from career + station training
    /// tours. `null` when the scene has no player. See
    /// `shock2vr::player_stats::PlayerStats`.
    pub stats: Option<shock2vr::player_stats::PlayerStats>,
    /// The audio logs the player has collected (frobbed), in pickup order.
    /// Persisted in `QuestInfo`; survives level transitions and save/load. See
    /// `shock2vr::quest_info::CollectedLog`.
    pub collected_logs: Vec<shock2vr::quest_info::CollectedLog>,
    /// The automap locations explored in the current mission (ascending).
    /// Persisted per mission in `QuestInfo`; survives save/load.
    pub explored_map_locations: Vec<i32>,
}

/// Input state snapshot
#[derive(Debug, Serialize, Clone)]
pub struct InputSnapshot {
    pub head_rotation: [f32; 4], // Quaternion [x, y, z, w]
    pub hands: HandsSnapshot,
}

/// Hand input state
#[derive(Debug, Serialize, Clone)]
pub struct HandsSnapshot {
    pub left: HandSnapshot,
    pub right: HandSnapshot,
}

/// Individual hand state
#[derive(Debug, Serialize, Clone)]
pub struct HandSnapshot {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion [x, y, z, w]
    pub thumbstick: [f32; 2],
    pub trigger: f32,
    pub squeeze: f32,
    pub a: f32,
}
