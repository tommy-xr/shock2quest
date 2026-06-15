import { HttpClient } from "./client.js";
import type {
  CommandResult,
  DebugEntityMessage,
  EntityDetailResult,
  EntityListResult,
  FrameSnapshot,
  InputAction,
  PathfindingTestStatus,
  Position,
  RayCastRequest,
  RayCastResult,
  ScreenshotResult,
  StepResult,
  StepSpec,
  TeleportResult,
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

  constructor(protected readonly client: HttpClient) {
    this.player = new PlayerApi(client);
    this.entities = new EntitiesApi(client);
    this.input = new InputApi(client);
    this.pathfindingTest = new PathfindingTestApi(client);
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
