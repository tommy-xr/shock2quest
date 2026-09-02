import { HttpClient } from "./client.js";
import type {
  AnimationState,
  CommandResult,
  DebugEntityMessage,
  DevParamSetResult,
  CameraPlacement,
  CameraState,
  DevParamsListResult,
  EntityDetailResult,
  EntityListResult,
  EntitySummary,
  FrameSnapshot,
  InputAction,
  PathfindingStats,
  PathfindingTestStatus,
  PhysicsBodyDetail,
  PhysicsBodyListResult,
  SceneListResult,
  SceneObjectSummary,
  Position,
  RagdollMetricsResult,
  ClimbGripResult,
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
  RecentMessagesResult,
  UiState,
  PlayerInventoryResult,
  PlayerStats,
  PlayerStatsRequest,
  SpawnedItem,
  TransitionsResult,
  WaitForOptions,
  AiPathEntry,
  PathRouteResult,
  AimOptions,
  AimPoint,
  AimResult,
  AimVisibility,
  Vec3,
} from "./types.js";

/**
 * Standing eye height above the player's body position, in world units:
 * the Rust `PLAYER_EYE_HEIGHT` (head sphere at `(PLAYER_HEIGHT / 2) -
 * PLAYER_RADIUS` = 1.8 SS2 ft, plus the original game's 0.8 ft eye offset =
 * 2.6 SS2 ft, i.e. 5.6 ft above the floor) divided by `dark::SCALE_FACTOR`
 * (2.5). Only a fallback: aiming prefers the live `camera_offset` the runtime
 * reports in `/v1/info`.
 */
export const PLAYER_EYE_HEIGHT_WORLD = 1.04;

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
   * first use the nearest visible selectable surface on the center ray,
   * including explicit `center` requests. Occluded targets and unavailable
   * classifications gracefully fall back to the authored entity position.
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
    const eyeHeight =
      options?.eyeHeight ?? snapshot.player.camera_offset?.[1] ?? PLAYER_EYE_HEIGHT_WORLD;
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

    // An explicit `center` on a classified creature keeps authored-center
    // semantics; ordinary objects resolve `center` to their visible surface.
    const explicitCreatureCenter = requested === "center" && aimPoints.length > 0;
    let surfacePoint: Vec3 | undefined;
    let interactionTargetId: number | null = null;
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
          interactionTargetId = check.hit.entity_id;
          surfacePoint = check.hit.hit_point;
        }
      } else if (!explicitCreatureCenter) {
        const surface = await this.client.post<RayCastResult>("/v1/physics/raycast", {
          start: eye,
          end: detail.position,
          collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
          // Match the production interaction ray, which ignores trigger sensors.
          ignore_sensors: true,
        });
        interactionTargetId = surface?.entity_id ?? null;
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
        !explicitCreatureCenter &&
        (requested === "center"
          ? surfacePoint === undefined
          : requested !== "surface" || surfacePoint === undefined),
      visibility: visibility ?? {
        state: "unchecked",
        origin: "view",
        target_distance: distance(worldPoint),
        blocker: null,
      },
      interaction_target_id: interactionTargetId,
      target_confirmed: interactionTargetId === entityId,
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
   * entities.byTemplate). Throws (400) if the entity is not alive or is held
   * by a container; container loot must go through the real MFD. World items
   * land in the backpack; wielding is a separate, presentation-specific step.
   */
  async give(entityId: number): Promise<CommandResult> {
    return this.client.post<CommandResult>("/v1/player/give", {
      entity_id: entityId,
    });
  }

  /**
   * Debug provisioning: instantiate an item template and put it straight into
   * the player's backpack, as if it had been picked up. Address it by gamesys
   * template name (`"Shotgun"`, case-insensitive) or by template id (`-19`) -
   * templates are the only identity stable across runs.
   *
   * Only genuine pickup items are accepted; a creature or door template throws
   * (400) and nothing is left in the world. Wielding a provisioned weapon is a
   * separate step (double-click it in the use-mode inventory strip).
   */
  async spawnItem(template: string | number): Promise<SpawnedItem> {
    return this.client.post<SpawnedItem>(
      "/v1/player/spawn-item",
      typeof template === "number" ? { template_id: template } : { template },
    );
  }

  /**
   * Debug provisioning: raise the character sheet to the requested levels
   * (stats, skills, psi tier, cyber modules) through the same `PlayerStats`
   * mutations a trainer purchase performs, free of charge. Every field is
   * optional and names the level to establish; omitted fields are untouched.
   * Only raises - a target below the current level throws (400), as does one
   * above the cap (stats/skills 6, psi tier 5). Returns the resulting sheet.
   */
  async setStats(request: PlayerStatsRequest): Promise<PlayerStats> {
    return this.client.post<PlayerStats>("/v1/player/stats", request);
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

  /**
   * Press a discrete action and KEEP it held, so a hold-to-activate button can
   * be driven without a controller (the Menu button's long press). Step to let
   * the hold accumulate, then `release`.
   */
  async hold(action: InputAction): Promise<CommandResult> {
    return unwrap(
      await this.client.post<CommandResult>("/v1/input/action", {
        action,
        hold: true,
      }),
    );
  }

  /** Release an action held by `hold`. */
  async release(action: InputAction): Promise<CommandResult> {
    return unwrap(
      await this.client.post<CommandResult>("/v1/input/action", {
        action,
        hold: false,
      }),
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

  /** Hold or release the ordinary production jump button. */
  async setJump(pressed: boolean): Promise<void> {
    await this.set("jump", pressed ? 1 : 0);
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
    const eyeHeight =
      options?.eyeHeight ?? snapshot.player.camera_offset?.[1] ?? PLAYER_EYE_HEIGHT_WORLD;
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

/**
 * World rotation whose -Z axis points along `direction`, with the horizon kept
 * level (zero roll) for every direction.
 *
 * Built as yaw-then-pitch rather than as a shortest-arc quaternion: the
 * shortest arc from -Z has no roll control, so directions near world +z came
 * out rolled up to ~180 degrees - which silently corrupts screenshots and
 * skews strafing, since locomotion moves along the camera's right axis.
 *
 * Exported because the same hazard applies to any rig whose orientation carries
 * an offset: a spurious roll turns "up out of the fist" into "sideways".
 */
export function lookQuat(direction: Vec3): Quat {
  const length = Math.hypot(...direction);
  if (length === 0) throw new Error("look-at target must differ from the player's eye position");
  const [dx, dy, dz] = direction.map((value) => value / length) as Vec3;
  // Straight up/down leaves yaw undefined; pin it to 0 so the result is stable.
  const horizontal = Math.hypot(dx, dz);
  const yaw = horizontal === 0 ? 0 : Math.atan2(-dx, -dz);
  const pitch = Math.atan2(dy, horizontal);
  const [sy, cy] = [Math.sin(yaw / 2), Math.cos(yaw / 2)];
  const [sp, cp] = [Math.sin(pitch / 2), Math.cos(pitch / 2)];
  // yaw about world +Y, then pitch about the yawed +X: no roll by construction.
  return [cy * sp, sy * cp, -sy * sp, cy * cp];
}

/** Pure look-at transform, exported so callers can verify/control custom rigs. */
export function headRotationForWorldPoint(
  playerPosition: Vec3,
  playerRotation: Quat,
  target: Vec3,
  eyeHeight = PLAYER_EYE_HEIGHT_WORLD,
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

  /**
   * One body in full, including how many contacts it currently has and which
   * bodies those contacts are with (`contacts`). The
   * endpoint answers `null` for an unknown body id, which this reports as an
   * error rather than handing back a null that only fails later.
   */
  async body(bodyId: number): Promise<PhysicsBodyDetail> {
    const detail = await this.client.get<PhysicsBodyDetail | null>(
      `/v1/physics/bodies/${bodyId}`,
    );
    if (detail === null) {
      throw new Error(`no physics body with id ${bodyId}`);
    }
    return detail;
  }

  /** Per-ragdoll settle/quality metrics (empty list when no ragdolls exist). */
  async ragdolls(): Promise<RagdollMetricsResult> {
    return this.client.get<RagdollMetricsResult>("/v1/ragdoll/metrics");
  }

  /**
   * What a hand at `point` could grab: an authored ladder face, a walkable
   * ledge above the player's feet, or nothing. `feetY` defaults to the
   * player's own feet height.
   */
  async grip(
    point: Vec3,
    options?: { radius?: number; feetY?: number },
  ): Promise<ClimbGripResult> {
    const params = new URLSearchParams({
      x: String(point[0]),
      y: String(point[1]),
      z: String(point[2]),
    });
    if (options?.radius !== undefined)
      params.set("radius", String(options.radius));
    if (options?.feetY !== undefined) params.set("feet_y", String(options.feetY));
    return this.client.get<ClimbGripResult>(`/v1/physics/grip?${params}`);
  }
}

/** What the renderer was handed on the last frame (GET /v1/scene). */
export class SceneApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * The objects submitted to the renderer, optionally scoped to one entity or
   * to the transparent draws. Use `source` to identify a render path -
   * "player_hands", "frontend_pointer", "pause_dim" and friends.
   */
  async objects(options?: {
    entityId?: number;
    transparent?: boolean;
    limit?: number;
  }): Promise<SceneListResult> {
    const params = new URLSearchParams();
    if (options?.entityId !== undefined)
      params.set("entity_id", String(options.entityId));
    if (options?.transparent !== undefined)
      params.set("transparent", String(options.transparent));
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const query = params.size > 0 ? `?${params}` : "";
    return this.client.get<SceneListResult>(`/v1/scene${query}`);
  }

  /** The objects a given render path produced this frame. */
  async fromSource(source: string): Promise<SceneObjectSummary[]> {
    const { objects } = await this.objects();
    return objects.filter((object) => object.source === source);
  }
}

/** Live-tunable developer parameters (the dev_params registry mirror). */
export class DevParamsApi {
  constructor(private readonly client: HttpClient) {}

  /** Every registered param with its range, current value, and default. */
  async list(): Promise<DevParamsListResult> {
    return this.client.get<DevParamsListResult>("/v1/dev-params");
  }

  /**
   * Set one param by key. The runtime clamps into the param's range and snaps
   * to its step grid; the result reports the value actually applied. Consumers
   * read the registry every frame, so the change is live on the next stepped
   * frame. Unknown keys reject with HTTP 404.
   */
  async set(key: string, value: number): Promise<DevParamSetResult> {
    return this.client.post<DevParamSetResult>("/v1/dev-params", { key, value });
  }

  /**
   * Restore one param to its exact declared default. A `set(key, default)`
   * cannot always get there - the snap grid does not round-trip every
   * default - so reset is its own operation.
   */
  async reset(key: string): Promise<DevParamSetResult> {
    return this.client.post<DevParamSetResult>("/v1/dev-params", {
      key,
      reset: true,
    });
  }
}

/** The free (debug) camera - a detached view that leaves the pawn alone. */
export class CameraApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * Whether the camera is detached, and the pose it is rendering from. Lets a
   * test assert what the camera did without reading pixels: that it held its
   * pose while the player walked, flew where it was told, and re-attached on
   * a level change.
   */
  async state(): Promise<CameraState> {
    return this.client.get<CameraState>("/v1/camera");
  }

  /**
   * Detach the camera and place it in the world - the only way to photograph
   * something the player's own eye cannot see, the player themselves included.
   *
   * `{ position, lookAt }` is the primitive worth reaching for: stand the
   * camera off to one side and aim it at the thing under test. The returned
   * state reports where the eye ended up, so a caller can assert on the pose it
   * asked for rather than trusting the request.
   *
   * The simulation is untouched: the pawn does not move, and everything that
   * reads the player's position keeps reading the body. Placing does turn the
   * `free_camera` developer option on (nothing else would keep the placement
   * past the next step), and while the camera is detached the pawn will not
   * walk - the locomotion sticks fly the camera instead.
   *
   * Aim the head AFTER this and the camera swings off its target: the runtime
   * composes the tracked head onto the camera pose, and this divides out the
   * head as it is now. Place the camera last, or re-issue the placement.
   */
  async set(placement: CameraPlacement): Promise<CameraState> {
    return this.client.post<CameraState>("/v1/camera", {
      detached: true,
      position: placement.position,
      look_at: placement.lookAt,
      rotation: placement.rotation,
    });
  }

  /** Return the view to the player's own eye. */
  async attach(): Promise<CameraState> {
    return this.client.post<CameraState>("/v1/camera", { detached: false });
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
   * The most recently played sounds (oldest first): resolved schema sample,
   * query tags, world position, sim time/frame, clip duration, source entity
   * and audio handle. Snapshot the last `sequence` before an action, then
   * filter for higher sequences to find the sounds that action played.
   */
  async recent(): Promise<RecentAudioResult> {
    return this.client.get<RecentAudioResult>("/v1/audio/recent");
  }
}

/**
 * Script message trace - what actually drove script behavior on a frame.
 * Pairs with {@link AudioApi} for debugging "why did all of this fire at once".
 */
export class MessagesApi {
  constructor(private readonly client: HttpClient) {}

  /**
   * The most recently delivered script messages (oldest first). High-frequency
   * payloads (hover, sensor intersect, collision) are filtered out so the
   * buffer holds useful event history. Damage entries include their physical
   * impact direction/point when the source supplied them.
   */
  async recent(): Promise<RecentMessagesResult> {
    return this.client.get<RecentMessagesResult>("/v1/messages/recent");
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

  /**
   * Does a walk route exist between two world positions? Answers "the AI is
   * failing to route" vs "nothing walkable connects these at all". Null when
   * the scene has no pathfinding data.
   */
  async route(from: Vec3, to: Vec3): Promise<PathRouteResult | null> {
    const q = (v: Vec3) => v.join(",");
    try {
      return await this.client.get<PathRouteResult>(
        `/v1/pathfinding/route?from=${q(from)}&to=${q(to)}`,
      );
    } catch {
      return null;
    }
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
  readonly scene: SceneApi;
  readonly devParams: DevParamsApi;
  readonly camera: CameraApi;
  readonly quests: QuestsApi;
  readonly ui: UiApi;
  readonly audio: AudioApi;
  readonly messages: MessagesApi;

  constructor(protected readonly client: HttpClient) {
    this.player = new PlayerApi(client);
    this.entities = new EntitiesApi(client);
    this.input = new InputApi(client);
    this.pathfindingTest = new PathfindingTestApi(client);
    this.pathfinding = new PathfindingApi(client);
    this.physics = new PhysicsApi(client);
    this.scene = new SceneApi(client);
    this.devParams = new DevParamsApi(client);
    this.camera = new CameraApi(client);
    this.quests = new QuestsApi(client);
    this.ui = new UiApi(client);
    this.audio = new AudioApi(client);
    this.messages = new MessagesApi(client);
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

  /**
   * Capture the current frame. Saved at the runtime's declared 800x600 by
   * default (even on a HiDPI framebuffer); pass `maxWidth` for more detail.
   */
  async screenshot(filename?: string, maxWidth?: number): Promise<ScreenshotResult> {
    return this.client.post<ScreenshotResult>("/v1/screenshot", {
      filename,
      max_width: maxWidth,
    });
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
   * Rejects with HTTP 409 while the live player is dead, unsupported/falling,
   * or in transient locomotion that cannot be represented safely. That JSON
   * error body includes `error_code`, `reason`, and the exact `player_pose`.
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
