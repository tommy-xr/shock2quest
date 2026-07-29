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
/** One AI's most recent path query (GET /v1/ai/paths). */
export interface AiPathEntry {
  /** EntityId::inner() as i32 - same id space as the entity endpoints. */
  entity_id: number;
  /** Where the AI was trying to go. */
  goal: [number, number, number];
  /** "Full", "Partial" (closest reachable), or "Failed". */
  outcome: string;
  /** Route waypoints (empty for Failed). */
  waypoints: [number, number, number][];
}

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
  | {
      type: "Damage";
      amount: number;
      /** Optional world-space blow direction (seeds a death-ragdoll reaction). */
      direction?: [number, number, number];
      /** Optional world-space hit point (defaults to the victim's position). */
      point?: [number, number, number];
    }
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
  /**
   * Live classified creature hitboxes. Optional for compatibility with debug
   * runtimes predating the aim-point capability.
   */
  aim_points?: AimPoint[];
}

export interface AimPoint {
  proxy_entity_id: number;
  body_id: number;
  joint_id: number;
  classification: "head" | "torso" | "limb" | "extremity" | "no_damage";
  position: Vec3;
}

export type AimClassification =
  | "head"
  | "torso"
  | "limb"
  | "surface"
  | "center"
  | "nearest";

export interface AimOptions {
  hitbox?: AimClassification;
  /** Override the runtime's live camera height, in world units. */
  eyeHeight?: number;
  /**
   * `required` verifies line of sight from the flat camera eye and rejects an
   * occluded target. It does not claim clearance from the weapon muzzle.
   */
  visibility?: "unchecked" | "required";
}

export interface AimVisibilityBlocker {
  entity_id: number | null;
  entity_name: string | null;
  body_id: number | null;
  collision_group: string | null;
  hit_point: Vec3 | null;
  distance: number | null;
}

export interface AimVisibility {
  state: "unchecked" | "visible" | "blocked";
  origin: "view";
  target_distance: number;
  blocker: AimVisibilityBlocker | null;
}

export interface AimResult {
  entity_id: number;
  proxy_entity_id: number | null;
  body_id: number | null;
  joint_id: number | null;
  requested: AimClassification;
  classification: AimPoint["classification"] | "surface" | "center";
  world_point: Vec3;
  head_rotation: Quat;
  fallback_used: boolean;
  /** Classification fallback and visibility are independent. */
  visibility: AimVisibility;
  /** Production-equivalent interaction-ray hit used to choose the aim point. */
  interaction_target_id: number | null;
  /** Whether that interaction ray selected the requested runtime entity. */
  target_confirmed: boolean;
}

/** A clip queued behind the currently-playing head clip. */
export interface AnimationQueueEntry {
  name: string | null;
  num_frames: number;
  looping: boolean;
}

/** An in-flight crossfade from a previous clip's pose. */
export interface AnimationBlend {
  from_clip: string | null;
  from_frame: number;
  /** Total blend duration (seconds). */
  duration: number;
  /** Time elapsed into the blend (seconds). */
  elapsed: number;
  /** Blend progress 0..1 (0 = fully the old pose). */
  alpha: number;
}

/**
 * Animation playback state + world-space posed skeleton for one entity, as
 * reported by GET /v1/entities/:id/animation (null when the entity has no
 * animation player).
 */
export interface AnimationState {
  entity_id: number;
  /** Currently playing clip (queue head), null when the queue is empty. */
  clip: string | null;
  /** Current frame within the head clip. */
  frame: number;
  num_frames: number;
  looping: boolean;
  /** Sub-frame time carried toward the next frame advance (seconds). */
  remaining_time: number;
  /** Clips queued behind the head, in play order. */
  queue: AnimationQueueEntry[];
  /** Clip whose final frame poses the skeleton while the queue is empty. */
  last_clip: string | null;
  blend: AnimationBlend | null;
  /** Entity world position / rotation (the pose the joints are composed with). */
  position: Vec3;
  rotation: Quat;
  /**
   * World-space joint positions (fixed 40-slot skeleton; unused slots track
   * the entity transform).
   */
  joints: Vec3[];
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
  ignore_sensors?: boolean;
}

export interface RayCastResult {
  hit: boolean;
  hit_point: Vec3 | null;
  hit_normal: Vec3 | null;
  distance: number | null;
  entity_id: number | null;
  entity_name: string | null;
  /** Optional when connected to runtimes predating blocker-body reporting. */
  body_id?: number | null;
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
  is_sleeping: boolean;
}

export interface PhysicsBodyListResult {
  bodies: PhysicsBodySummary[];
  total_count: number;
  player_position: Vec3;
}

/** Settle/quality metrics for one ragdoll (GET /v1/ragdoll/metrics). */
export interface RagdollMetrics {
  entity_id: number;
  body_count: number;
  max_linear_speed: number;
  max_angular_speed: number;
  min_y: number;
  max_nonadjacent_overlap: number;
  max_drift: number;
}

export interface RagdollMetricsResult {
  ragdolls: RagdollMetrics[];
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
  /** The player's persistent character sheet (primary stats, trained skills,
   * mastered psi disciplines), accumulated from career + station training
   * tours; null when the scene has no player. */
  stats: PlayerStats | null;
  /** The audio logs the player has collected (frobbed), in pickup order.
   * Persisted in QuestInfo; survives level transitions and save/load. */
  collected_logs: CollectedLog[];
  /** The automap locations explored in the current mission (ascending).
   * Persisted per mission in QuestInfo; survives save/load. */
  explored_map_locations: number[];
}

/** One audio log the player has collected, keyed by its per-deck identity. */
export interface CollectedLog {
  deck: number;
  log: number;
}

/** Trainable skill levels (weapon proficiencies + tech skills). */
export interface SkillLevels {
  standard_weapons: number;
  energy_weapons: number;
  heavy_weapons: number;
  exotic_weapons: number;
  hack: number;
  repair: number;
  modify: number;
  maintenance: number;
  research: number;
}

/** The player's persistent character sheet. Primary stats start at a baseline
 * of 1 and skills at 0; station training tours raise them per the (career,
 * year, tour) reward table. `psi_disciplines` lists OSA-mastered disciplines by
 * display name; `granted_years` records which training years were applied. */
export interface PlayerStats {
  strength: number;
  endurance: number;
  agility: number;
  psionic_ability: number;
  cyber_affinity: number;
  skills: SkillLevels;
  psi_disciplines: string[];
  granted_years: number[];
  /** Cyber modules: the game's upgrade currency, awarded by quest/`PropExp`
   * traps and module pickups, spent at trainer stations. Persists across level
   * transitions and save/load. Defaults to 0 for saves written before the
   * currency existed. */
  cyber_modules: number;
  /** Highest psi tier unlocked at a psi trainer (0..=5, sequential). */
  psi_tier: number;
  /** O/S upgrade traits acquired at trait machines, by retail trait id
   * (1..=16, TRAITS.STR order), in acquisition order; at most 4. */
  os_traits: number[];
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

/** An item provisioned into the backpack by `POST /v1/player/spawn-item`. */
export interface SpawnedItem {
  /** Runtime entity id of the fresh item (same id space as /v1/entities). */
  entity_id: number;
  /** The template it was instantiated from - stable across runs. */
  template_id: number;
  name: string | null;
}

/** Debug provisioning target for the character sheet (`POST /v1/player/stats`).
 * Mirrors the read-side `player.stats` shape; every field is optional and names
 * the level to establish (not a delta). Provisioning only raises: a target
 * below the current level is rejected with a 400. */
export interface PlayerStatsRequest {
  strength?: number;
  endurance?: number;
  agility?: number;
  psionic_ability?: number;
  cyber_affinity?: number;
  skills?: Partial<SkillLevels>;
  /** Highest psi tier unlocked (0..=5, granted sequentially). */
  psi_tier?: number;
  /** Target cyber-module balance (the upgrade currency). */
  cyber_modules?: number;
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
  /**
   * The AMMOFULL ammo-type cycle button, exposed only in use mode when a gun
   * with 2+ ammo types is wielded (the expanded weapon panel, flat UI 5).
   * Click its `screen_rect` center to cycle the wielded weapon's ammo type
   * (Effect::CycleAmmo). null in shooter mode / unarmed / single-ammo weapons.
   */
  ammo_cycle: UiElement | null;
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
