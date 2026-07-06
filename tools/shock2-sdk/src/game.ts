import { HttpClient } from "./client.js";
import type {
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
  PlayerInventoryResult,
  TransitionsResult,
  WaitForOptions,
} from "./types.js";

/** Error thrown when the game reports a command failed (success: false). */
export class CommandError extends Error {
  constructor(public readonly result: CommandResult) {
    super(result.message);
    this.name = "CommandError";
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

  constructor(protected readonly client: HttpClient) {
    this.player = new PlayerApi(client);
    this.entities = new EntitiesApi(client);
    this.input = new InputApi(client);
    this.pathfindingTest = new PathfindingTestApi(client);
    this.pathfinding = new PathfindingApi(client);
    this.physics = new PhysicsApi(client);
    this.quests = new QuestsApi(client);
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
