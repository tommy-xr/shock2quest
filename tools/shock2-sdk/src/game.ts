import { HttpClient } from "./client.js";
import type {
  AnimationState,
  CommandResult,
  DebugEntityMessage,
  EntityDetailResult,
  EntityListResult,
  EntitySummary,
  FrameSnapshot,
  InputAction,
  PathfindingStats,
  PathfindingTestStatus,
  PhysicsBodyListResult,
  Position,
  RagdollMetricsResult,
  RayCastRequest,
  RayCastResult,
  ScreenshotResult,
  StepResult,
  StepSpec,
  MoveResult,
  TeleportResult,
  TransitionLevelResult,
  SaveLoadResult,
  QuestBitsResult,
  QuestBitValue,
  RecentAudioResult,
  UiState,
  PlayerInventoryResult,
  TransitionsResult,
  WaitForOptions,
  AiPathEntry,
  AimOptions,
  AimPoint,
  AimResult,
  AimVisibility,
  Vec3,
} from "./types.js";

/** Error thrown when the game reports a command failed (success: false). */
export class CommandError extends Error {
  constructor(public readonly result: CommandResult) {
    super(result.message);
    this.name = "CommandError";
  }
}

/** Error thrown when `aimAt(..., { visibility: "required" })` is occluded. */
export class AimOcclusionError extends Error {
  constructor(public readonly result: AimResult) {
    const blocker = result.visibility.blocker;
    const label =
      blocker?.entity_name ??
      (blocker?.entity_id === null ? "world geometry" : `entity ${blocker?.entity_id}`);
    super(`view from camera eye to entity ${result.entity_id} is blocked by ${label}`);
    this.name = "AimOcclusionError";
  }
}

function unwrap(result: CommandResult): CommandResult {
  if (!result.success) {
    throw new CommandError(result);
  }
  return result;
}

/** Player-related queries and controls. */
export class PlayerApi {
  constructor(private readonly client: HttpClient) {}

  async position(): Promise<Position> {
    const result = await this.client.get<{ position: [number, number, number] }>(
      "/v1/player/position",
    );
    const [x, y, z] = result.position;
    return { x, y, z };
  }

  async teleport(position: Position): Promise<TeleportResult> {
    return this.client.post<TeleportResult>("/v1/player/teleport", position);
  }

  /**
   * Move the player toward `target` in a single bounded, collision-valid hop.
   *
   * The displacement is clamped to at most 5 world units and the player
   * collider is shape-cast along the way, stopping just short of any geometry
   * it hits (`blocked: true`). Unlike {@link teleport}, this can never move the
   * player through a wall or out of bounds - prefer it for exploration. A
   * closed door blocks the move, so the pattern is: move up to the door (it
   * trips the tripwire, or Frob it), step until it opens, then move through.
   *
   * Named `moveTo` because `move` is awkward as a bare method name.
   */
  async moveTo(target: Position): Promise<MoveResult> {
    return this.client.post<MoveResult>("/v1/player/move", target);
  }

  /**
   * Aim the production flat camera, interaction ray, and weapon at an entity.
   *
   * Creature targets use their live classified damage proxies. Other entities
   * first use the nearest visible selectable surface on the center ray.
   * Occluded targets and unavailable classifications gracefully fall back to
   * the entity center. Request `center` to aim at the origin explicitly.
   *
   * This accounts for authored and save-restored pawn rotation; raw
   * `head.look` / `head.rotation` are pawn-local.
   */
  async aimAt(
    entity: number | Pick<EntitySummary, "id">,
    options?: AimOptions,
  ): Promise<AimResult> {
    const entityId = typeof entity === "number" ? entity : entity.id;
    const requested = options?.hitbox ?? "torso";
    const [detail, snapshot] = await Promise.all([
      this.client.get<EntityDetailResult>(`/v1/entities/${entityId}`),
      this.client.get<FrameSnapshot>("/v1/info"),
    ]);
    const eyeHeight = options?.eyeHeight ?? 1.6;
    const eye: Vec3 = [
      snapshot.player.position[0],
      snapshot.player.position[1] + eyeHeight,
      snapshot.player.position[2],
    ];
    const distance = (point: Vec3) =>
      Math.hypot(point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]);

    // Entity detail is a versioned runtime capability. Older runtimes (and a
    // stale binary encountered by the campaign) omit aim_points entirely;
    // aiming must remain usable and report its center fallback, not crash.
    const aimPoints = detail.aim_points ?? [];
    let candidates = [...aimPoints];
    if (requested === "head" || requested === "torso") {
      candidates = candidates.filter((point) => point.classification === requested);
    } else if (requested === "limb") {
      candidates = candidates.filter(
        (point) =>
          point.classification === "limb" || point.classification === "extremity",
      );
    } else if (requested === "center" || requested === "surface") {
      candidates = [];
    }
    candidates.sort((a, b) => distance(a.position) - distance(b.position));
    const targetIds = new Set([
      entityId,
      ...aimPoints.map((point) => point.proxy_entity_id),
    ]);
    const checkViewVisibility = async (
      point: Vec3,
    ): Promise<{ visibility: AimVisibility; hit: RayCastResult }> => {
      const hit = await this.client.post<RayCastResult>("/v1/physics/raycast", {
        start: eye,
        end: point,
        collision_groups: [
          "entity",
          "hitbox",
          "selectable",
          "world",
          "ui",
          "raycast",
        ],
        // Match production interaction/projectile rays: enclosing tripwire and
        // room sensors are not physical visibility blockers.
        ignore_sensors: true,
      });
      const visible = !hit.hit || (hit.entity_id !== null && targetIds.has(hit.entity_id));
      return {
        hit,
        visibility: {
          state: visible ? "visible" : "blocked",
          origin: "view",
          target_distance: distance(point),
          blocker: visible
            ? null
            : {
                entity_id: hit.entity_id,
                entity_name: hit.entity_name,
                body_id: hit.body_id ?? null,
                collision_group: hit.collision_group,
                hit_point: hit.hit_point,
                distance: hit.distance,
              },
        },
      };
    };

    let selected: AimPoint | undefined = candidates[0];
    let visibility: AimVisibility | undefined;
    if (options?.visibility === "required" && candidates.length > 0) {
      selected = undefined;
      for (const candidate of candidates) {
        const check = await checkViewVisibility(candidate.position);
        visibility ??= check.visibility;
        if (check.visibility.state === "visible") {
          selected = candidate;
          visibility = check.visibility;
          break;
        }
      }
      // Preserve the closest requested-class target in the structured error.
      selected ??= candidates[0];
    }

    let surfacePoint: Vec3 | undefined;
    if (candidates.length === 0) {
      if (options?.visibility === "required") {
        const check = await checkViewVisibility(detail.position);
        visibility = check.visibility;
        if (
          check.visibility.state === "visible" &&
          check.hit.entity_id !== null &&
          targetIds.has(check.hit.entity_id) &&
          check.hit.hit_point !== null
        ) {
          surfacePoint = check.hit.hit_point;
        }
      } else if (requested !== "center") {
        const surface = await this.client.post<RayCastResult>("/v1/physics/raycast", {
          start: eye,
          end: detail.position,
          collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
        });
        if (surface?.entity_id === entityId && surface.hit_point !== null) {
          surfacePoint = surface.hit_point;
        }
      }
    }

    const worldPoint = selected?.position ?? surfacePoint ?? detail.position;
    const headRotation = headRotationForWorldPoint(
      snapshot.player.position,
      snapshot.player.rotation,
      worldPoint,
      eyeHeight,
    );
    const result: AimResult = {
      entity_id: entityId,
      proxy_entity_id: selected?.proxy_entity_id ?? null,
      body_id: selected?.body_id ?? null,
      joint_id: selected?.joint_id ?? null,
      requested,
      classification: selected?.classification ?? (surfacePoint ? "surface" : "center"),
      world_point: worldPoint,
      head_rotation: headRotation,
      fallback_used:
        selected === undefined &&
        requested !== "center" &&
        (requested !== "surface" || surfacePoint === undefined),
      visibility: visibility ?? {
        state: "unchecked",
        origin: "view",
        target_distance: distance(worldPoint),
        blocker: null,
      },
    };
    if (result.visibility.state === "blocked") {
      throw new AimOcclusionError(result);
    }
    await this.client.post("/v1/control/input", {
      channel: "head.rotation",
      value: headRotation,
    });
    return result;
  }

  /** The items the player is carrying (backpack + hand-held), for verifying pickups. */
  async inventory(): Promise<PlayerInventoryResult> {
    return this.client.get<PlayerInventoryResult>("/v1/player/inventory");
  }

  /**
   * Put an existing world entity into the player's inventory - a headless
   * "pick up" for tests. `entityId` is a runtime id (see entities.list /
   * entities.byTemplate). Throws (400) if the entity is not alive. The item
   * lands in the backpack; wielding is a separate, presentation-specific step.
   */
  async give(entityId: number): Promise<CommandResult> {
    return this.client.post<CommandResult>("/v1/player/give", {
      entity_id: entityId,
    });
  }
}

/** Entity listing and inspection. */
export class EntitiesApi {
  constructor(private readonly client: HttpClient) {}

  async list(options?: { filter?: string; limit?: number }): Promise<EntityListResult> {
    const params = new URLSearchParams();
    if (options?.filter !== undefined) params.set("filter", options.filter);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const query = params.size > 0 ? `?${params}` : "";
    return this.client.get<EntityListResult>(`/v1/entities${query}`);
  }

  async detail(id: number): Promise<EntityDetailResult> {
    return this.client.get<EntityDetailResult>(`/v1/entities/${id}`);
  }

  /**
   * Runtime entities instantiated from a given template id (the negative ids
   * from dark_query / gamesys). Maps template -> runtime id, e.g. to find the
   * wrench in a level before giving it to the player. Lists ALL entities (no
   * limit, so nothing is truncated by the distance-sorted cap) and filters by
   * template.
   */
  async byTemplate(templateId: number): Promise<EntitySummary[]> {
    const { entities } = await this.list();
    return entities.filter((e) => e.template_id === templateId);
  }

  /**
   * Animation playback state + world-space posed skeleton for an entity:
   * current clip / frame, queued clips, in-flight blend, and the 40
   * world-space joint positions. Returns null when the entity has no
   * animation player. Sample once per stepped frame to quantify pose
   * smoothness (see src/anim-metrics.ts).
   */
  async animation(id: number): Promise<AnimationState | null> {
    return this.client.get<AnimationState | null>(
      `/v1/entities/${id}/animation`,
    );
  }

  /**
   * Inject a script message into an entity (damage, frob, AI signal).
   *
   * The message is queued and delivered on the next step(); throws if the
   * entity is not found or not alive.
   */
  async sendMessage(
    id: number,
    message: DebugEntityMessage,
  ): Promise<CommandResult> {
    return unwrap(
      await this.client.post<CommandResult>(
        `/v1/entities/${id}/message`,
        message,
      ),
    );
  }
}

/** Discrete input actions and continuous input channels. */
export class InputApi {
  constructor(private readonly client: HttpClient) {}

  /** Trigger a discrete action (as if its bound key was pressed). Applies on the next update. */
  async trigger(action: InputAction): Promise<CommandResult> {
    return unwrap(
      await this.client.post<CommandResult>("/v1/input/action", { action }),
    );
  }

  /** List action names accepted by trigger(). */
  async actions(): Promise<string[]> {
    const result = await this.client.get<{ actions: string[] }>("/v1/input/actions");
    return result.actions;
  }

  /** Set a continuous input channel, e.g. set("right_hand.trigger_value", 1.0). */
  async set(channel: string, value: unknown): Promise<void> {
    await this.client.post("/v1/control/input", { channel, value });
  }

  /**
   * Point the production flat-mode camera/interaction ray at a world position.
   *
   * Unlike the low-level `head.look` and `head.rotation` channels, this method
   * accounts for the player's authored or save-restored body rotation. The
   * resulting head rotation still flows through the normal camera,
   * FlatInteraction, and weapon paths; this is aiming, not a debug hit shim.
   *
   * `eyeHeight` defaults to the flat player's standing eye height. Pass a
   * different value when deliberately testing crouched aiming.
   */
  async lookAtWorldPoint(target: Vec3, options?: { eyeHeight?: number }): Promise<void> {
    const [{ position }, snapshot] = await Promise.all([
      this.client.get<{ position: Vec3 }>("/v1/player/position"),
      this.client.get<FrameSnapshot>("/v1/info"),
    ]);
    const eyeHeight = options?.eyeHeight ?? 1.6;
    const localHeadRotation = headRotationForWorldPoint(
      position,
      snapshot.player.rotation,
      target,
      eyeHeight,
    );
    await this.set("head.rotation", localHeadRotation);
  }
}

type Quat = [number, number, number, number];

function multiplyQuat(a: Quat, b: Quat): Quat {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

function lookQuat(direction: Vec3): Quat {
  const length = Math.hypot(...direction);
  if (length === 0) throw new Error("look-at target must differ from the player's eye position");
  const [bx, by, bz] = direction.map((value) => value / length) as Vec3;
  const dot = -bz;
  if (dot < -0.999999) return [0, 1, 0, 0];
  const quaternion: Quat = [by, -bx, 0, 1 + dot];
  const quaternionLength = Math.hypot(...quaternion);
  return quaternion.map((value) => value / quaternionLength) as Quat;
}

/** Pure look-at transform, exported so callers can verify/control custom rigs. */
export function headRotationForWorldPoint(
  playerPosition: Vec3,
  playerRotation: Quat,
  target: Vec3,
  eyeHeight = 1.6,
): Quat {
  const worldLook = lookQuat([
    target[0] - playerPosition[0],
    target[1] - (playerPosition[1] + eyeHeight),
    target[2] - playerPosition[2],
  ]);
  const inversePawn: Quat = [
    -playerRotation[0],
    -playerRotation[1],
    -playerRotation[2],
    playerRotation[3],
  ];
  return multiplyQuat(inversePawn, worldLook);
}

/** Physics rigid-body inspection (positions, velocities). */
export class PhysicsApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * List rigid bodies, optionally scoped to one entity id (a single entity
   * can own several bodies, e.g. ragdoll limbs).
   */
  async bodies(options?: {
    entityId?: number;
    limit?: number;
  }): Promise<PhysicsBodyListResult> {
    const params = new URLSearchParams();
    if (options?.entityId !== undefined)
      params.set("entity_id", String(options.entityId));
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const query = params.size > 0 ? `?${params}` : "";
    return this.client.get<PhysicsBodyListResult>(`/v1/physics/bodies${query}`);
  }

  /** Per-ragdoll settle/quality metrics (empty list when no ragdolls exist). */
  async ragdolls(): Promise<RagdollMetricsResult> {
    return this.client.get<RagdollMetricsResult>("/v1/ragdoll/metrics");
  }
}

/** Interactive pathfinding test (visual A* debugging). */
export class PathfindingTestApi {
  constructor(private readonly client: HttpClient) {}

  /** Advance the test state machine (set start -> set goal -> reset), like the desktop P key. */
  async cycle(): Promise<CommandResult> {
    return unwrap(
      await this.client.post<CommandResult>("/v1/pathfinding-test", {
        action: "cycle",
      }),
    );
  }

  async status(): Promise<PathfindingTestStatus> {
    return this.client.get<PathfindingTestStatus>("/v1/pathfinding-test");
  }
}

/** Quest bits (objective flags): read progress, or set for test setup. */
export class QuestsApi {
  constructor(private readonly client: HttpClient) {}

  /** Snapshot every quest bit the game has set. Bits never touched are absent (they read as "unknown"). */
  async list(): Promise<QuestBitsResult> {
    return this.client.get<QuestBitsResult>("/v1/quests");
  }

  /** Value of a single quest bit by name ("unknown" if the game hasn't set it). */
  async get(name: string): Promise<QuestBitValue> {
    const { quests } = await this.list();
    const lower = name.toLowerCase();
    return quests.find((q) => q.name === lower)?.value ?? "unknown";
  }

  /** Set a quest bit (test setup / skipping ahead). Throws on an invalid value. */
  async set(name: string, value: QuestBitValue): Promise<CommandResult> {
    return this.client.post<CommandResult>(
      `/v1/quests/${encodeURIComponent(name)}`,
      { value },
    );
  }
}

/** Flat-mode UI state introspection (GET /v1/ui). */
export class UiApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * Current UI snapshot: mode is "shooter" or "use" (Tab metagame mode), and
   * the open MFD panel (frob a keypad/container to open one) with its
   * labeled, clickable elements. Click an element by setting the
   * `pointer.position` channel to the center of its `screen_rect`, then
   * pulsing `pointer.pressed`.
   */
  async state(): Promise<UiState> {
    return this.client.get<UiState>("/v1/ui");
  }
}

/** Played-sound introspection (there is no other headless way to observe audio). */
export class AudioApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * The most recently played environmental sounds (oldest first): resolved
   * schema sample, query tags, and world position. Snapshot the last
   * `sequence` before an action, then filter for higher sequences to find
   * the sounds that action played.
   */
  async recent(): Promise<RecentAudioResult> {
    return this.client.get<RecentAudioResult>("/v1/audio/recent");
  }
}

/** Pathfinding service telemetry (distinct from the interactive test). */
export class PathfindingApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * Monotonic pathfinding query counters, or null when the scene has no
   * pathfinding data. Diff snapshots across steps to measure per-frame load.
   * Counts every find_path caller, including unbudgeted ones (e.g. the
   * interactive pathfinding test).
   */
  async stats(): Promise<PathfindingStats | null> {
    return this.client.get<PathfindingStats | null>("/v1/pathfinding/stats");
  }

  /**
   * The latest route each AI computed: goal, waypoints, and whether the
   * query found a Full route, a Partial (closest-reachable) route, or
   * Failed. Empty until an AI has pathed.
   */
  async aiPaths(): Promise<AiPathEntry[]> {
    return this.client.get<AiPathEntry[]>("/v1/ai/paths");
  }
}

/**
 * Connected handle to a running debug runtime.
 *
 * The game starts paused; call step() to advance the simulation. Injected
 * input actions are consumed by the next update (which runs even while
 * paused, with zero delta time).
 */
export class Game {
  readonly player: PlayerApi;
  readonly entities: EntitiesApi;
  readonly input: InputApi;
  readonly pathfindingTest: PathfindingTestApi;
  readonly pathfinding: PathfindingApi;
  readonly physics: PhysicsApi;
  readonly quests: QuestsApi;
  readonly ui: UiApi;
  readonly audio: AudioApi;

  constructor(protected readonly client: HttpClient) {
    this.player = new PlayerApi(client);
    this.entities = new EntitiesApi(client);
    this.input = new InputApi(client);
    this.pathfindingTest = new PathfindingTestApi(client);
    this.pathfinding = new PathfindingApi(client);
    this.physics = new PhysicsApi(client);
    this.quests = new QuestsApi(client);
    this.ui = new UiApi(client);
    this.audio = new AudioApi(client);
  }

  get baseUrl(): string {
    return this.client.baseUrl;
  }

  async health(): Promise<{ status: string }> {
    return this.client.get<{ status: string }>("/v1/health");
  }

  async info(): Promise<FrameSnapshot> {
    return this.client.get<FrameSnapshot>("/v1/info");
  }

  /** Advance the simulation by frames ({ frames: 10 }) or time ({ duration: "1s" }). */
  async step(spec: StepSpec = { frames: 1 }): Promise<StepResult> {
    return this.client.post<StepResult>("/v1/step", spec);
  }

  async screenshot(filename?: string): Promise<ScreenshotResult> {
    return this.client.post<ScreenshotResult>("/v1/screenshot", { filename });
  }

  async raycast(request: RayCastRequest): Promise<RayCastResult> {
    return this.client.post<RayCastResult>("/v1/physics/raycast", request);
  }

  /**
   * Level-transition triggers in the current scene (where each leads + its
   * position). Teleport into a trigger's position and step to let the real
   * trigger fire the transition, instead of warping with transitionLevel().
   */
  async transitions(): Promise<TransitionsResult> {
    return this.client.get<TransitionsResult>("/v1/transitions");
  }

  /**
   * Warp to another level (as if an in-game transition trigger fired), letting
   * a tester jump directly to any mission in isolation. `level` may omit the
   * ".mis" suffix; `loc` is an optional spawn-marker id (map default if omitted).
   * Without the loading_screen feature the switch is synchronous and info()
   * reflects the new mission immediately.
   */
  async transitionLevel(
    level: string,
    loc?: number,
  ): Promise<TransitionLevelResult> {
    return this.client.post<TransitionLevelResult>(
      "/v1/control/transition-level",
      { level, loc },
    );
  }

  /**
   * Save the current game to a named file (a bare name, no extension / path
   * separators - e.g. "frontier"). Persists the active mission, player
   * position/rotation, quest bits, and held items. The save survives across
   * runtime relaunches, so a later `load(file)` in a fresh session resumes it.
   */
  async save(file: string): Promise<SaveLoadResult> {
    return this.client.post<SaveLoadResult>("/v1/save", { file });
  }

  /**
   * Load a previously-saved game by name, restoring the active mission, player
   * position/rotation, quest bits, and held items. The restore is synchronous,
   * so info() reflects the loaded mission immediately. Rejects (404) if no save
   * with that name exists.
   */
  async load(file: string): Promise<SaveLoadResult> {
    return this.client.post<SaveLoadResult>("/v1/load", { file });
  }

  /**
   * Poll until predicate returns a truthy value; resolves with that value.
   * Throws on timeout, including the description in the error message.
   */
  async waitFor<T>(
    predicate: () => Promise<T> | T,
    options?: WaitForOptions,
  ): Promise<NonNullable<T>> {
    const timeoutMs = options?.timeoutMs ?? 10_000;
    const intervalMs = options?.intervalMs ?? 100;
    const deadline = Date.now() + timeoutMs;
    let lastValue: T | undefined;

    for (;;) {
      lastValue = await predicate();
      if (lastValue) {
        return lastValue as NonNullable<T>;
      }
      if (Date.now() >= deadline) {
        const what = options?.description ?? "condition";
        throw new Error(
          `Timed out after ${timeoutMs}ms waiting for ${what} (last value: ${JSON.stringify(lastValue)})`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }

  /** Request a graceful shutdown of the debug runtime. */
  async shutdown(): Promise<void> {
    await this.client.post("/v1/shutdown");
  }
}
