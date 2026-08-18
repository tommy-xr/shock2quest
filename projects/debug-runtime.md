# Debug Runtime

## Project Status: 🟡 IN PROGRESS (~70% Complete)

An HTTP-controlled game runtime that enables LLMs and automation scripts to remotely "play" and debug shock2vr without human interaction.

## Architecture

- **`runtimes/debug_runtime`** - HTTP server + game loop with remote control
- **`tools/debug_command`** - CLI tool for sending commands (placeholder)
- **`DebuggableScene` trait** - Debug API implemented by Mission scenes

The debug runtime binds to `127.0.0.1:8080` (localhost only) and provides a REST API for controlling the game. Commands are processed via an async channel between the HTTP server (tokio/axum) and the main game loop (GLFW/OpenGL).

## Implementation Status

### Phase 1: Foundation ✅ COMPLETE

| Task                                           | Status |
| ---------------------------------------------- | ------ |
| Project structure setup                        | ✅     |
| Cargo aliases (`dbgr`, `dbgc`)                 | ✅     |
| HTTP server with axum                          | ✅     |
| Localhost-only binding                         | ✅     |
| Health check endpoint                          | ✅     |
| Game initialization                            | ✅     |
| GLFW window + OpenGL rendering                 | ✅     |
| Command-line args (mission, port, debug flags) | ✅     |
| Basic CLI tool structure                       | ✅     |

### Phase 2: Core Commands ✅ COMPLETE

| Task                               | Status |
| ---------------------------------- | ------ |
| Command channel (mpsc)             | ✅     |
| Frame stepping (`/v1/step`)        | ✅     |
| Time-based stepping (humantime)    | ✅     |
| Game state info (`/v1/info`)       | ✅     |
| Shutdown endpoint (`/v1/shutdown`) | ✅     |

### Phase 3: Entity System ✅ COMPLETE

| Task                                    | Status |
| --------------------------------------- | ------ |
| `DebuggableScene` trait                 | ✅     |
| Entity listing (`/v1/entities`)         | ✅     |
| Entity detail (`/v1/entities/{id}`)     | ✅     |
| Message injection (`/v1/entities/{id}/message`) | ✅ |
| Name filtering with wildcards           | ✅     |
| Distance-based sorting                  | ✅     |
| Player position (`/v1/player/position`) | ✅     |
| Player teleport (`/v1/player/teleport`) | ✅     |

### Phase 4: Physics & Input ✅ MOSTLY COMPLETE

| Task                                            | Status |
| ----------------------------------------------- | ------ |
| Raycast (`/v1/physics/raycast`)                 | ✅     |
| Physics body listing (`/v1/physics/bodies`)     | ✅     |
| Physics body detail (`/v1/physics/bodies/{id}`) | ✅     |
| Impulse joint diagnostics (`/v1/physics/joints`) | ✅    |
| Per-ragdoll metrics (`/v1/ragdoll/metrics`)     | ✅     |
| Screenshot capture (`/v1/screenshot`)           | ✅     |
| macOS Retina scaling fix                        | ✅     |
| Input state read (`/v1/control/input`)          | ✅     |
| Input state write (`/v1/control/input` POST)    | ✅     |
| Multi-level testing (earth.mis, medsci2.mis)    | ✅     |

**Deterministic stepping (2026-06-18):** `/v1/step` advances at a fixed 60 Hz
timestep, so `{"frames":N}` == N/60 s of sim time and `{"duration":T}` runs
exactly T·60 frames, independent of HTTP request timing. (Previously it used the
real wall-clock dt between requests, which made physics-settle measurements —
e.g. a ragdoll falling — erratic.) `/v1/physics/joints` reports each impulse
joint's anchor separation + applied impulse (bone-labeled) for ragdoll
constraint diagnostics.

### Phase 5: Game Commands 🟡 PARTIAL

Dedicated typed endpoints rather than one stringly-typed command dispatcher -
the `/v1/control/command` placeholder was removed as dead API surface.

| Task                                                  | Status |
| ----------------------------------------------------- | ------ |
| Debug provisioning (`/v1/player/spawn-item`, `/v1/player/stats`) | ✅     |
| Save/load commands (`/v1/save`, `/v1/load`)           | ✅     |
| Level transition (`/v1/control/transition-level`)     | ✅     |
| God mode, noclip                                      | ❌     |

### Phase 6: TypeScript API ✅ COMPLETE

A Playwright-inspired TypeScript/JavaScript SDK for driving the game programmatically. Enables LLMs to write test scripts and automation without dealing with raw HTTP. See `tools/shock2-sdk/README.md` for usage.

**Location:** `tools/shock2-sdk/` (npm package, zero runtime dependencies)

| Task                          | Status |
| ----------------------------- | ------ |
| Package setup (TypeScript)    | ✅     |
| `GameServer.launch()` API     | ✅ (health polling, log capture, `await using` auto-shutdown) |
| `game.step()` / `game.waitFor()` | ✅  |
| `game.player` accessors       | ✅     |
| `game.entities` query API     | ✅ (`list({filter, limit})`, `detail(id)`) |
| `game.screenshot()`           | ✅     |
| `game.input.*` controls       | ✅ (discrete actions + continuous channels) |
| `game.pathfindingTest.*`      | ✅ (cycle + status verification) |
| Auto-spawn debug_runtime      | ✅     |
| Connection retry/reconnect    | ✅ (`GameServer.connect()`) |
| E2E scenario test             | ✅ (`npm run test:e2e` replays the pathfinding scenario) |

#### API Design

```typescript
import { GameServer } from '@shock2vr/sdk';

// Launch game server (spawns debug_runtime process)
const game = await GameServer.launch({
  mission: 'medsci1.mis',
  port: 8080,
  experimental: ['teleport'],
});

// Or connect to existing server
const game = await GameServer.connect('http://127.0.0.1:8080');

// Step simulation
await game.step({ frames: 10 });
await game.step({ duration: '5s' });

// Player control
const pos = await game.player.position();
await game.player.teleport({ x: 10, y: 2, z: 15 });

// Entity queries (chainable, like Playwright locators)
const doors = await game.entities.filter('*Door*').list();
const camera = await game.entities.byId(445).detail();
const nearby = await game.entities.nearPlayer(10).list();

// Input simulation
await game.input.rightHand.trigger(1.0);
await game.input.leftHand.thumbstick(0.5, -0.8);
await game.input.head.rotation([0, 0.707, 0, 0.707]);

// Screenshots
const screenshot = await game.screenshot('test.png');
console.log(`Saved to ${screenshot.fullPath}`);

// Physics queries
const hit = await game.raycast({
  start: [0, 0, 0],
  end: [10, 0, 0],
  collision_groups: ['world', 'entity'],
});

// Cleanup
await game.shutdown();
```

#### Test Script Example

```typescript
import { GameServer, expect } from '@shock2vr/sdk';

describe('Camera AI', () => {
  let game: GameServer;

  beforeAll(async () => {
    game = await GameServer.launch({ mission: 'medsci2.mis' });
  });

  afterAll(async () => {
    await game.shutdown();
  });

  it('should detect player in line of sight', async () => {
    // Find a security camera
    const camera = await game.entities.filter('*Camera*').first();

    // Teleport player in front of camera
    await game.player.teleport({ x: 8, y: 3, z: 12 });

    // Step simulation to let camera detect
    await game.step({ duration: '2s' });

    // Check camera state changed
    const detail = await game.entities.byId(camera.id).detail();
    expect(detail.properties).toContainEqual({
      name: 'AI_Alertness',
      value: expect.stringContaining('High'),
    });
  });
});
```

#### Implementation Notes

- Uses `fetch()` internally to call debug_runtime HTTP API
- `GameServer.launch()` spawns `cargo dbgr` as child process
- Waits for `/v1/health` endpoint before resolving
- Auto-cleanup on process exit (SIGINT handler)
- TypeScript types generated from Rust command structs
- Publish to npm as `@shock2vr/sdk` or similar

### Phase 7: CLI Tool 🔴 NOT STARTED

| Task                        | Status |
| --------------------------- | ------ |
| `debug_command info`        | ❌     |
| `debug_command adv`         | ❌     |
| `debug_command ls`          | ❌     |
| `debug_command ent`         | ❌     |
| `debug_command rc`          | ❌     |
| `debug_command ss`          | ❌     |
| `--raw` / `--pretty` output | ❌     |

## Available HTTP Endpoints

```
GET  /v1/health           - Health check
GET  /v1/info             - Game state snapshot
POST /v1/step             - Step simulation (frames or duration)
POST /v1/shutdown         - Graceful shutdown
GET  /v1/entities         - List entities (with ?limit=N&filter=pattern)
GET  /v1/entities/{id}    - Entity details
POST /v1/entities/{id}/message - Inject a script message (damage/frob/signal)
GET  /v1/player/position  - Player position
POST /v1/player/teleport  - Teleport player
POST /v1/physics/raycast  - Physics raycast
GET  /v1/physics/bodies   - List physics bodies
GET  /v1/physics/bodies/{id} - Physics body details
GET  /v1/control/input    - Get input state
POST /v1/control/input    - Set input channel
POST /v1/player/spawn-item - Provision an item template into the inventory
POST /v1/player/stats     - Provision skills/stats/psi tier/cyber modules
POST /v1/screenshot       - Capture screenshot
GET  /v1/dev-params       - List live-tunable dev params (key, label, range, value, default)
POST /v1/dev-params       - Set a dev param {key, value}; clamped + snapped, 404 on unknown key
```

`/v1/dev-params` mirrors the `shock2vr::dev_params` registry (the live tuning
knobs the Developer menu will expose - frontend panel distance, pause dim
strength, eye-height offset). The values are process-global atomics read by
their consumers every frame, so the handlers touch the registry directly - no
`RuntimeCommand` round-trip - and a POST is visible on the next stepped frame:

```bash
curl http://127.0.0.1:8080/v1/dev-params
curl -X POST http://127.0.0.1:8080/v1/dev-params -d '{"key": "panel_distance", "value": 3.0}'
```

The `/v1/control/input` channel vocabulary lives in `shock2vr::input::remote`
(not in this runtime), because the oculus runtime speaks the same channels for
on-device automation - see the `vr-device-loop` skill. There it is applied as an
override on top of the OpenXR-built `InputContext` (`InputOverrides`); here the
runtime owns the context and patches it directly.

## Usage

### Starting the Debug Runtime

```bash
# Basic usage
cargo dbgr -- --mission medsci1.mis --port 8080

# With debug flags
cargo dbgr -- --mission earth.mis --debug-physics --debug-draw

# With experimental features
cargo dbgr -- --mission medsci1.mis --experimental teleport
```

### API Examples

```bash
# Health check
curl http://127.0.0.1:8080/v1/health

# Step 10 frames
curl -X POST http://127.0.0.1:8080/v1/step \
  -H "Content-Type: application/json" \
  -d '{"frames": 10}'

# Step 5 seconds
curl -X POST http://127.0.0.1:8080/v1/step \
  -H "Content-Type: application/json" \
  -d '{"duration": "5s"}'

# List entities
curl "http://127.0.0.1:8080/v1/entities?limit=20&filter=*Door*"

# Inject a script message into an entity (body is a tagged DebugEntityMessage:
# {"type":"Damage","amount":N} | {"type":"Frob"} | {"type":"Signal","name":"..."})
curl -X POST http://127.0.0.1:8080/v1/entities/122/message \
  -H "Content-Type: application/json" \
  -d '{"type": "Damage", "amount": 5.0}'

# Teleport player
curl -X POST http://127.0.0.1:8080/v1/player/teleport \
  -H "Content-Type: application/json" \
  -d '{"x": 10.0, "y": 2.0, "z": 15.0}'

# Raycast
curl -X POST http://127.0.0.1:8080/v1/physics/raycast \
  -H "Content-Type: application/json" \
  -d '{"start": [0,0,0], "end": [10,0,0], "collision_groups": ["world", "entity"]}'

# Screenshot
curl -X POST http://127.0.0.1:8080/v1/screenshot \
  -H "Content-Type: application/json" \
  -d '{"filename": "test.png"}'
```

Omitting `collision_groups` defaults to `["world", "entity"]`. Supported
names are `world`, `entity`, `selectable`, `player`, `ui`, `hitbox`, `raycast`,
and `all`; the older documented name `level` is accepted as an alias for
`world`. Unknown names and explicit empty masks return HTTP 400. Sensors are
ignored by default so room triggers do not look like distance-zero occluders;
pass `"ignore_sensors": false` to probe sensor volumes deliberately.

## Key Files

| File                                     | Purpose                               |
| ---------------------------------------- | ------------------------------------- |
| `runtimes/debug_runtime/src/main.rs`     | HTTP server + game loop (~1600 lines) |
| `runtimes/debug_runtime/src/commands.rs` | Command/response types (~390 lines)   |
| `tools/debug_command/src/main.rs`        | CLI tool (placeholder, ~48 lines)     |
| `shock2vr/src/game_scene.rs`             | `DebuggableScene` trait definition    |
| `shock2vr/src/mission/mission_core.rs`   | `DebuggableScene` implementation      |

## Next Steps

1. **[Keybinding System Refactor](keybinding-system.md)** - Prerequisite for Phase 5
   - Centralizes input handling in shock2vr
   - Enables `/v1/input/action` endpoint to trigger any game action
   - Eliminates `Command` trait (direct `InputAction` → `Effect` mapping)
2. **Phase 5: Game Commands** - Implement spawn, save/load, level transition (after keybinding refactor)
3. **Phase 6: TypeScript SDK** - Playwright-style API for LLM scripting
4. **Phase 7: CLI Tool** - Build out `debug_command` with all subcommands
5. **Error Handling** - Standardize error responses with codes and suggestions
6. **Documentation** - Add OpenAPI spec and usage examples
7. **Aimable debug camera** - Let callers point the debug camera at an arbitrary
   world position/orientation, so agents can frame whatever they're inspecting
   (e.g. wherever a ragdoll lands) instead of relying on a fixed default view.
   Either a new `/v1/camera` endpoint (set position + look-at target, or
   position + rotation) or by honoring the head rotation already present in
   `POST /v1/control/input`. Today the debug runtime hardcodes the camera head
   rotation to the desktop default (`default_camera_head_rotation()` in
   `runtimes/debug_runtime/src/main.rs`, looking toward -X); screenshots are only
   representative when the subject happens to be in that default view.

## Technical Notes

- Game starts **paused** by default - use `/v1/step` to advance
- Screenshots saved to `/tmp/claude/` directory
- Input overrides persist until reset
- Frame counter tracks actual game frames (not wall time)
- macOS Retina displays: viewport size auto-detected for correct screenshots

## Input Action Injection ✅ COMPLETE

The [Keybinding System Refactor](keybinding-system.md) resolved the previous
"Command Integration Complexity" architectural issue. Discrete input actions
are now defined centrally in `shock2vr::input::InputAction` and can be
injected over HTTP:

```bash
# List available actions
curl http://127.0.0.1:8080/v1/input/actions

# Trigger any action (as if the bound key was pressed)
curl -X POST http://127.0.0.1:8080/v1/input/action \
  -H "Content-Type: application/json" \
  -d '{"action": "PathfindingTestCycle"}'

# Pathfinding test: trigger a cycle (POST) and read back state (GET)
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test \
  -H "Content-Type: application/json" -d '{"action": "cycle"}'
curl http://127.0.0.1:8080/v1/pathfinding-test
# => {"state":"WaitingForGoal","test_path_waypoints":0}
```

Injected actions are consumed by the next `game.update()` (they apply even
while paused, since the paused loop runs zero-time updates). The pathfinding
test GET endpoint exists specifically so HTTP clients can *verify* that a
triggered action executed, without scraping logs.

Adding a new debug action is now trivial:
1. Add a variant to `shock2vr/src/input/actions.rs` (and its `all()`/`as_str()` entries)
2. Map it to an `Effect` in `shock2vr/src/input/dispatcher.rs`
3. It is immediately triggerable via `/v1/input/action` and bindable to keys
   via `DesktopInputMapper`
