// Command processing for debug runtime
//
// This module defines the command interface between the HTTP server and game loop,
// allowing remote control of the running game through a request/response pattern.

use cgmath::Vector3;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Commands that can be sent from HTTP handlers to the game loop
#[derive(Debug)]
pub enum RuntimeCommand {
    /// Get current game state snapshot
    GetInfo(oneshot::Sender<FrameSnapshot>),

    /// Step the simulation forward by frames or time
    Step(StepSpec, oneshot::Sender<StepResult>),

    /// Take a screenshot of the current frame
    Screenshot(ScreenshotSpec, oneshot::Sender<ScreenshotResult>),

    /// Perform a physics raycast
    RayCast(RayCastRequest, oneshot::Sender<RayCastResult>),

    /// Get current input state
    GetInput(oneshot::Sender<InputState>),

    /// Set input channel values
    SetInput(InputPatch),

    /// Move the player to a position
    MovePlayer(Vector3<f32>),

    /// Get current player position
    GetPlayerPosition(oneshot::Sender<Vector3<f32>>),

    /// Look at an entity or position
    LookAt(LookAtRequest, oneshot::Sender<LookAtResult>),

    /// Check visibility of an entity or position
    VisibilityCheck(VisibilityRequest, oneshot::Sender<VisibilityResult>),

    /// Perform a shape intersection test
    ShapeCast(ShapeCastRequest, oneshot::Sender<ShapeCastResult>),

    /// Execute a game command (spawn, save, etc.)
    RunGameCommand(String, Vec<String>, oneshot::Sender<CommandResult>),

    /// Pathfinding test command (set_start, set_goal, reset)
    PathfindingTest(String, oneshot::Sender<CommandResult>),

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

    /// List physics rigid bodies
    ListPhysicsBodies {
        limit: Option<usize>,
        reply: oneshot::Sender<PhysicsBodyListResult>,
    },

    /// Get detailed information about a physics body
    PhysicsBodyDetail {
        id: u32,
        reply: oneshot::Sender<Option<PhysicsBodyDetailResult>>,
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
    pub collision_group: Option<String>,
    pub is_sensor: bool,
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
    pub is_sensor: bool,
    pub is_enabled: bool,
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
    pub target_id: i32,
    pub target_name: String,
}

/// Current state of the game
#[derive(Debug, Serialize, Clone)]
pub struct FrameSnapshot {
    pub frame_index: u64,
    pub time: TimeInfo,
    pub mission: String,
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
    pub position: [f32; 3],
    pub rotation: [f32; 4], // quaternion
    pub camera_offset: [f32; 3],
    pub camera_rotation: [f32; 4], // quaternion
}

/// Input state snapshot
#[derive(Debug, Serialize, Clone)]
pub struct InputSnapshot {
    pub head_rotation: [f32; 4], // quaternion
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
    pub rotation: [f32; 4], // quaternion
    pub thumbstick: [f32; 2],
    pub trigger: f32,
    pub squeeze: f32,
    pub a: f32,
}

impl FrameSnapshot {
    /// Create a new frame snapshot with default values
    pub fn new() -> Self {
        Self {
            frame_index: 0,
            time: TimeInfo {
                elapsed_ms: 0.0,
                total_ms: 0.0,
            },
            mission: "none".to_string(),
            player: PlayerInfo {
                entity_id: None,
                position: [0.0, 0.0, 0.0],
                rotation: [1.0, 0.0, 0.0, 0.0],
                camera_offset: [0.0, 0.0, 0.0],
                camera_rotation: [1.0, 0.0, 0.0, 0.0],
            },
            entity_count: 0,
            debug_features: vec![],
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
}

// ============================================================================
// Look-At Request/Response Types
// ============================================================================

/// Request for looking at an entity or position
#[derive(Debug, Deserialize)]
pub struct LookAtRequest {
    /// Entity ID to look at (mutually exclusive with position)
    pub entity_id: Option<i32>,
    /// World position to look at (mutually exclusive with entity_id)
    pub position: Option<[f32; 3]>,
    /// Optional offset from entity position (e.g., [0.0, 1.5, 0.0] for eye level)
    #[serde(default)]
    pub offset: Option<[f32; 3]>,
}

/// Result of look-at operation
#[derive(Debug, Serialize)]
pub struct LookAtResult {
    pub success: bool,
    pub message: String,
    /// The world position that was looked at
    pub target_position: Option<[f32; 3]>,
    /// The new head rotation quaternion [x, y, z, w]
    pub new_head_rotation: Option<[f32; 4]>,
    /// Distance from player to target
    pub distance: Option<f32>,
}

// ============================================================================
// Visibility Check Request/Response Types
// ============================================================================

/// Request for checking visibility of an entity or position
#[derive(Debug, Deserialize)]
pub struct VisibilityRequest {
    /// Entity ID to check visibility of (mutually exclusive with position)
    pub entity_id: Option<i32>,
    /// World position to check visibility of (mutually exclusive with entity_id)
    pub position: Option<[f32; 3]>,
    /// Field of view in degrees (default: 90)
    #[serde(default)]
    pub fov_degrees: Option<f32>,
}

/// Result of visibility check
#[derive(Debug, Serialize)]
pub struct VisibilityResult {
    /// Whether the target is visible (in FOV and not occluded)
    pub visible: bool,
    /// Whether the target is within the field of view
    pub in_fov: bool,
    /// Whether the target is blocked by geometry/entities
    pub occluded: bool,
    /// Angle from player forward vector to target (degrees)
    pub angle_from_center: f32,
    /// Distance from player to target
    pub distance: f32,
    /// Information about what is blocking visibility (if occluded)
    pub occlusion_hit: Option<OcclusionHit>,
}

/// Information about what is blocking visibility
#[derive(Debug, Serialize)]
pub struct OcclusionHit {
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub hit_point: [f32; 3],
    pub distance: f32,
}

// ============================================================================
// Shape Cast Request/Response Types
// ============================================================================

/// Request for shape intersection test
#[derive(Debug, Deserialize)]
pub struct ShapeCastRequest {
    /// World position to test
    pub position: [f32; 3],
    /// Shape type: "player", "capsule", "sphere"
    #[serde(default = "default_shape")]
    pub shape: String,
    /// Radius (used for capsule and sphere)
    #[serde(default)]
    pub radius: Option<f32>,
    /// Height (used for capsule)
    #[serde(default)]
    pub height: Option<f32>,
}

fn default_shape() -> String {
    "player".to_string()
}

/// Result of shape intersection test
#[derive(Debug, Serialize)]
pub struct ShapeCastResult {
    /// Whether the shape fits at the position without collisions
    pub fits: bool,
    /// List of collisions if the shape doesn't fit
    pub collisions: Vec<ShapeCollision>,
}

/// Information about a collision in shape cast
#[derive(Debug, Serialize)]
pub struct ShapeCollision {
    pub entity_id: Option<i32>,
    pub entity_name: Option<String>,
    pub collision_point: [f32; 3],
    pub penetration_depth: f32,
}
