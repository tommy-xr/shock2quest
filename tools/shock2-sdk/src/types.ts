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
  | { type: "Signal"; name: string };

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
   * when unarmed / single-ammo / melee. */
  wielded_ammo_type: string | null;
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

export interface WaitForOptions {
  /** Total time to wait in milliseconds (default 10_000). */
  timeoutMs?: number;
  /** Poll interval in milliseconds (default 100). */
  intervalMs?: number;
  /** Description used in the timeout error message. */
  description?: string;
}
