/** Discrete input actions known to the game (see shock2vr/src/input/actions.rs). */
export type InputAction =
  | "PathfindingTestCycle"
  | "QuickSave"
  | "QuickLoad"
  | "SpawnDebugItem"
  | "MoveInventory"
  | "CycleWeapon"
  | "CycleAmmo"
  | "Reload"
  | "CyclePsiPower"
  | "ToggleUseMode"
  // Escape hatch so newly-added actions are usable before the SDK is updated.
  | (string & {});

export type Vec3 = [number, number, number];
export type Quat = [number, number, number, number];

export interface Position {
  x: number;
  y: number;
  z: number;
}

export interface StepSpec {
  frames?: number;
  /** humantime duration string, e.g. "5s" or "250ms" */
  duration?: string;
}

export interface StepResult {
  frames_advanced: number;
  time_advanced: number;
  new_frame_index: number;
  new_total_time: number;
}

export interface CommandResult {
  success: boolean;
  message: string;
  data: unknown;
}

/** Monotonic pathfinding query counters (diff across steps for rates). */
export interface PathfindingStats {
  queries: number;
  stressed_retries: number;
  no_route: number;
}

export interface PathfindingTestStatus {
  state: "WaitingForStart" | "WaitingForGoal" | "ShowingPath" | "Unavailable";
  test_path_waypoints: number;
}

/**
 * A script message that can be injected into a specific entity.
 *
 * Mirrors the engine's `DebugEntityMessage`; the `type` field is the serde tag.
 */
export type DebugEntityMessage =
  | { type: "Damage"; amount: number }
  | { type: "Frob" }
  | { type: "Signal"; name: string }
  | { type: "SetAlertness"; level: "Lowest" | "Low" | "Moderate" | "High" }
  // Switch-link activate/deactivate - what a tripwire/button sends to its targets.
  | { type: "TurnOn" }
  | { type: "TurnOff" };

export interface EntitySummary {
  id: number;
  name: string;
  template_id: number;
  position: Vec3;
  distance: number;
  script_count: number;
  link_count: number;
}

export interface EntityListResult {
  entities: EntitySummary[];
  total_count: number;
  player_position: Vec3;
}

export interface PropertyInfo {
  name: string;
  value: string;
}

export interface LinkInfo {
  link_type: string;
  target_id: number;
  target_name: string;
}

export interface EntityDetailResult {
  entity_id: number;
  name: string;
  template_id: number;
  position: Vec3;
  rotation: Quat;
  inheritance_chain: string[];
  properties: PropertyInfo[];
  outgoing_links: LinkInfo[];
  incoming_links: LinkInfo[];
}

export interface ScreenshotResult {
  filename: string;
  full_path: string;
  resolution: [number, number];
  size_bytes: number;
}

export interface RayCastRequest {
  start: Vec3;
  end: Vec3;
  collision_groups?: string[];
  max_distance?: number;
}

export interface RayCastResult {
  hit: boolean;
  hit_point: Vec3 | null;
  hit_normal: Vec3 | null;
  distance: number | null;
  entity_id: number | null;
  entity_name: string | null;
  collision_group: string | null;
  is_sensor: boolean;
}

/** Summary of one rigid body, as reported by GET /v1/physics/bodies. */
export interface PhysicsBodySummary {
  body_id: number;
  entity_id: number | null;
  entity_name: string | null;
  body_type: "dynamic" | "static" | "kinematic";
  position: Vec3;
  rotation: Quat;
  mass: number | null;
  velocity: Vec3;
  angular_velocity: Vec3;
  collision_groups: string[];
  is_sensor: boolean;
  is_enabled: boolean;
}

export interface PhysicsBodyListResult {
  bodies: PhysicsBodySummary[];
  total_count: number;
  player_position: Vec3;
}

export interface PlayerSnapshot {
  entity_id: number | null;
  position: Vec3;
  rotation: [number, number, number, number];
  camera_offset: Vec3;
  camera_rotation: [number, number, number, number];
  /** The wielded/first-person weapon in flatscreen mode; null when unarmed. */
  wielded_entity_id: number | null;
  /** The entity held in the right hand (VR); null otherwise. */
  right_hand_entity_id: number | null;
  /** Whether the wielded weapon is mid-reload, plus the current viewmodel tilt
   * (degrees) and reload progress (0..1). false/0/0 when not reloading. */
  reloading: boolean;
  reload_pitch_deg: number;
  reload_progress: number;
  /** The wielded weapon's selected ammo type (e.g. "std"/"he"/"ap"), or null
   * when unarmed / melee (no projectile links). */
  wielded_ammo_type: string | null;
  /** The player's current / maximum hit points, or null when the player has
   * no health pool. Drained by psi burnout. */
  hit_points: number | null;
  max_hit_points: number | null;
  /** The player's current / maximum psi points, or null when the player has
   * no psi pool. */
  psi_points: number | null;
  max_psi_points: number | null;
  /** The gamesys name of the selected psi power (what the psi amp casts),
   * e.g. "Cryokinesis". Cycle with the CyclePsiPower input action. */
  selected_psi_power: string | null;
  /** The psi amp's hold-to-overload meter fill (0..1) and phase
   * ("charging" / "overloaded" / "burnout"), or null when idle. */
  psi_charge: number | null;
  psi_charge_phase: string | null;
  /** The gamesys names of the active sustained psi powers (e.g. "Inviso"),
   * in activation order; empty when none. */
  active_psi_powers: string[];
}

export interface FrameSnapshot {
  frame_index: number;
  time: { total: number; delta: number };
  mission: string;
  player: PlayerSnapshot;
  entity_count: number;
  debug_features: string[];
  inputs: unknown;
}

export interface TeleportResult {
  success: boolean;
  message: string;
  new_position: Vec3;
}

/** Result of a bounded, shape-cast-validated player move (`player.moveTo`). */
export interface MoveResult {
  /** Whether the player position actually changed. */
  moved: boolean;
  /** Whether the move hit geometry before the full clamped distance. */
  blocked: boolean;
  new_position: Vec3;
  /** How far the player actually advanced (world units). */
  distance_moved: number;
  /** The distance the move was allowed to attempt: `min(target distance, 5.0)`. */
  requested_distance: number;
}

export interface TransitionLevelResult {
  success: boolean;
  /** Scene name after the transition, e.g. "eng1.mis". */
  mission: string;
  message: string;
}

export interface SaveLoadResult {
  success: boolean;
  /** Bare save name (no extension) that was saved/loaded. */
  file: string;
  /** Active scene after the operation, e.g. "medsci1.mis". */
  mission: string;
  message: string;
}

/** The 3-state value of a quest bit (objective flag). */
export type QuestBitValue = "unknown" | "incomplete" | "complete";

export interface QuestBitEntry {
  name: string;
  /** Friendly 3-state projection of `bits` (COMPLETE takes precedence). */
  value: QuestBitValue;
  /** Exact raw flag value; use when the precise value matters (scripts compare by raw value). */
  bits: number;
}

export interface QuestBitsResult {
  quests: QuestBitEntry[];
  count: number;
}

/** Where a carried item is held. */
export type InventoryLocation = "inventory" | "left_hand" | "right_hand";

export interface InventoryItem {
  entity_id: number;
  name: string | null;
  location: InventoryLocation;
}

export interface PlayerInventoryResult {
  items: InventoryItem[];
  count: number;
}

/**
 * One drawn element of the active flat-mode MFD panel. `label` gives
 * clickable elements a semantic identity (keypad digits "0"-"9", "clear",
 * the host "close" button); `rect` is on the 640x480 virtual canvas and
 * `screen_rect` is the same rect in normalized [0,1] screen coordinates
 * (letterbox-corrected) - feed its center straight to the `pointer.position`
 * input channel.
 */
export interface UiElement {
  /** "button" (clickable), "image", or "text". */
  kind: "button" | "image" | "text";
  texture: string | null;
  text: string | null;
  label: string | null;
  /** For loot-panel item buttons: the contained item's runtime entity id
   * (NOT stable across runs - resolve identity via label / entity lookup). */
  entity_id: number | null;
  /** Canvas-space rect [x, y, w, h] (640x480 virtual canvas). */
  rect: [number, number, number, number];
  /** Normalized screen-space rect [x, y, w, h]. */
  screen_rect: [number, number, number, number];
}

/** The open flat-mode MFD panel (opened by frobbing a GUI-bearing entity). */
export interface UiPanel {
  /** Runtime entity id of the bound object (NOT stable across runs). */
  entity_id: number;
  /** Stable template id (mission-file object id for level entities). */
  template_id: number;
  name: string | null;
  elements: UiElement[];
}

/**
 * The item currently held on the cursor (the original's "cursor IS the item"
 * drag model, §2.4): lifting a strip item puts it here and empties its slot;
 * placing/throwing clears it. `null` when the cursor is empty.
 */
export interface UiCursor {
  /** Runtime entity id of the held item (NOT stable across runs). */
  entity_id: number;
  /** The item's symbolic name (e.g. "Wrench"), if any. */
  label: string | null;
}

/** Flat-mode UI snapshot (GET /v1/ui). */
export interface UiState {
  mode: "shooter" | "use";
  /** The open MFD panel, or null when none is open. */
  active_panel: UiPanel | null;
  /**
   * The top-docked inventory strip (Tab metagame mode): the player's carried
   * items as labeled, clickable elements. Present only in "use" mode; null in
   * shooter mode.
   */
  strip: UiPanel | null;
  /** The item held on the cursor mid-drag, or null when the cursor is empty. */
  cursor: UiCursor | null;
}

export interface TransitionEntry {
  entity_id: number;
  name: string | null;
  /** Destination mission (no ".mis" suffix, e.g. "eng1"). */
  dest_level: string;
  dest_loc: number | null;
  position: Vec3;
}

export interface TransitionsResult {
  transitions: TransitionEntry[];
  count: number;
}

/** One resolved-and-played environmental sound (GET /v1/audio/recent). */
export interface PlayedSound {
  /** Monotonically increasing id - diff against a snapshot to find new plays. */
  sequence: number;
  /** Resolved schema sample name without extension (e.g. "bulmet2"). */
  sample: string;
  /** The schema query's (tag, value) pairs (e.g. ["event", "collision"]). */
  tags: [string, string][];
  position: [number, number, number];
}

export interface RecentAudioResult {
  sounds: PlayedSound[];
}

export interface WaitForOptions {
  /** Total time to wait in milliseconds (default 10_000). */
  timeoutMs?: number;
  /** Poll interval in milliseconds (default 100). */
  intervalMs?: number;
  /** Description used in the timeout error message. */
  description?: string;
}
