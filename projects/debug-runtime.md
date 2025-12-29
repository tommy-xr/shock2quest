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

### Phase 3: Entity System ✅ MOSTLY COMPLETE

| Task                                    | Status                   |
| --------------------------------------- | ------------------------ |
| `DebuggableScene` trait                 | ✅                       |
| Entity listing (`/v1/entities`)         | ✅                       |
| Entity detail (`/v1/entities/{id}`)     | ⚠️ Bug: off-by-one ID    |
| Name filtering with wildcards           | ✅                       |
| Distance-based sorting                  | ✅                       |
| Player position (`/v1/player/position`) | ✅                       |
| Player teleport (`/v1/player/teleport`) | ✅                       |

### Phase 4: Physics & Input ✅ MOSTLY COMPLETE

| Task                                            | Status |
| ----------------------------------------------- | ------ |
| Raycast (`/v1/physics/raycast`)                 | ✅     |
| Physics body listing (`/v1/physics/bodies`)     | ✅     |
| Physics body detail (`/v1/physics/bodies/{id}`) | ✅     |
| Screenshot capture (`/v1/screenshot`)           | ✅     |
| macOS Retina scaling fix                        | ✅     |
| Input state read (`/v1/control/input`)          | ✅     |
| Input state write (`/v1/control/input` POST)    | ✅     |
| Multi-level testing (earth.mis, medsci2.mis)    | ✅     |

### Phase 5: Testing Primitives 🟡 IN PROGRESS

Essential primitives for autonomous testing - enabling LLMs to verify game state programmatically.

| Task                                          | Status                    |
| --------------------------------------------- | ------------------------- |
| Look-at endpoint (`/v1/player/look-at`)       | ⚠️ Bug: rotation not applied |
| Visibility check (`/v1/visibility/check`)     | ✅                        |
| Shape cast (`/v1/physics/shapecast`)          | ✅                        |
| Game command endpoint (`/v1/control/command`) | ❌ Placeholder |
| Spawn command                                 | ❌             |
| Save/load commands                            | ❌             |
| Level transition                              | ❌             |
| God mode, noclip                              | ❌             |

#### 5.1 Look-At Endpoint

**Purpose**: Point the player's view toward a specific entity or world position.

**Endpoint**: `POST /v1/player/look-at`

**Request**:
```json
// Look at entity by ID
{ "entity_id": 445 }

// Look at world position
{ "position": [10.0, 2.0, 15.0] }

// Look at entity with offset
{ "entity_id": 445, "offset": [0.0, 1.5, 0.0] }
```

**Response**:
```json
{
  "success": true,
  "target_position": [10.0, 2.0, 15.0],
  "new_head_rotation": [0.0, 0.707, 0.0, 0.707],
  "distance": 5.2
}
```

**Implementation Notes**:
- Calculates direction from player head position to target
- Sets `head.rotation` in InputContext to face the target
- For entities, use their current position (optionally with offset for eye-level)
- Should work with both entity IDs and raw positions

#### 5.2 Visibility Check Endpoint

**Purpose**: Verify if a target entity or position is visible to the player (in FOV and not occluded).

**Endpoint**: `POST /v1/visibility/check`

**Request**:
```json
// Check if entity is visible
{ "entity_id": 445 }

// Check if position is visible
{ "position": [10.0, 2.0, 15.0] }

// Check with custom FOV (default: 90 degrees)
{ "entity_id": 445, "fov_degrees": 120.0 }
```

**Response**:
```json
{
  "visible": true,
  "in_fov": true,
  "occluded": false,
  "angle_from_center": 23.5,
  "distance": 5.2,
  "occlusion_hit": null
}

// If occluded:
{
  "visible": false,
  "in_fov": true,
  "occluded": true,
  "angle_from_center": 23.5,
  "distance": 5.2,
  "occlusion_hit": {
    "entity_id": 123,
    "entity_name": "Wall",
    "hit_point": [8.0, 2.0, 12.0],
    "distance": 3.1
  }
}
```

**Implementation Notes**:
- FOV check: Calculate angle between player forward vector and direction to target
- Raycast: Cast ray from player eye position to target position
- If raycast hits something before reaching target distance, it's occluded
- Useful for testing AI detection, line-of-sight mechanics, etc.

#### 5.3 Shape Cast Endpoint

**Purpose**: Check if a position has enough clearance for the player (useful for testing spawn points, teleport destinations).

**Endpoint**: `POST /v1/physics/shapecast`

**Request**:
```json
// Check if player-sized capsule fits at position
{
  "position": [10.0, 2.0, 15.0],
  "shape": "player"  // Uses player's collision shape
}

// Check with custom capsule
{
  "position": [10.0, 2.0, 15.0],
  "shape": "capsule",
  "radius": 0.5,
  "height": 1.8
}

// Check with sphere
{
  "position": [10.0, 2.0, 15.0],
  "shape": "sphere",
  "radius": 0.5
}
```

**Response**:
```json
{
  "fits": true,
  "collisions": []
}

// If doesn't fit:
{
  "fits": false,
  "collisions": [
    {
      "entity_id": 123,
      "entity_name": "Wall",
      "collision_point": [10.2, 2.0, 15.0],
      "penetration_depth": 0.3
    }
  ]
}
```

**Implementation Notes**:
- Uses Rapier's intersection test (not a moving shape cast)
- Player shape: typically a capsule with ~0.5m radius, ~1.8m height
- Returns all colliding bodies, not just the first
- Useful for validating teleport destinations, spawn points, pathfinding waypoints

### Phase 6: TypeScript API 🔴 NOT STARTED

A Playwright-inspired TypeScript/JavaScript SDK for driving the game programmatically. Enables LLMs to write test scripts and automation without dealing with raw HTTP.

**Location:** `tools/shock2-sdk/` (npm package)

| Task                          | Status |
| ----------------------------- | ------ |
| Package setup (TypeScript)    | ❌     |
| `GameServer.launch()` API     | ❌     |
| `game.step()` / `game.wait()` | ❌     |
| `game.player` accessors       | ❌     |
| `game.entities` query API     | ❌     |
| `game.screenshot()`           | ❌     |
| `game.input.*` controls       | ❌     |
| Auto-spawn debug_runtime      | ❌     |
| Connection retry/reconnect    | ❌     |

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
const hit = await game.physics.raycast({
  start: [0, 0, 0],
  end: [10, 0, 0],
  groups: ['entity', 'level'],
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
GET  /v1/health             - Health check
GET  /v1/info               - Game state snapshot
POST /v1/step               - Step simulation (frames or duration)
POST /v1/shutdown           - Graceful shutdown
GET  /v1/entities           - List entities (with ?limit=N&filter=pattern)
GET  /v1/entities/{id}      - Entity details
GET  /v1/player/position    - Player position
POST /v1/player/teleport    - Teleport player
POST /v1/player/look-at     - Look at entity/position
POST /v1/visibility/check   - Check if target is visible
POST /v1/physics/raycast    - Physics raycast
POST /v1/physics/shapecast  - Shape intersection test
GET  /v1/physics/bodies     - List physics bodies
GET  /v1/physics/bodies/{id} - Physics body details
GET  /v1/control/input      - Get input state
POST /v1/control/input      - Set input channel
POST /v1/control/command    - Execute game command (placeholder)
POST /v1/screenshot         - Capture screenshot
```

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

# Teleport player
curl -X POST http://127.0.0.1:8080/v1/player/teleport \
  -H "Content-Type: application/json" \
  -d '{"x": 10.0, "y": 2.0, "z": 15.0}'

# Raycast
curl -X POST http://127.0.0.1:8080/v1/physics/raycast \
  -H "Content-Type: application/json" \
  -d '{"start": [0,0,0], "end": [10,0,0], "collision_groups": ["entity", "level"]}'

# Screenshot
curl -X POST http://127.0.0.1:8080/v1/screenshot \
  -H "Content-Type: application/json" \
  -d '{"filename": "test.png"}'
```

## Key Files

| File                                     | Purpose                               |
| ---------------------------------------- | ------------------------------------- |
| `runtimes/debug_runtime/src/main.rs`     | HTTP server + game loop (~1600 lines) |
| `runtimes/debug_runtime/src/commands.rs` | Command/response types (~390 lines)   |
| `tools/debug_command/src/main.rs`        | CLI tool (placeholder, ~48 lines)     |
| `shock2vr/src/game_scene.rs`             | `DebuggableScene` trait definition    |
| `shock2vr/src/mission/mission_core.rs`   | `DebuggableScene` implementation      |

## Next Steps

1. **Phase 5: Game Commands** - Implement spawn, save/load, level transition
2. **Phase 6: TypeScript SDK** - Playwright-style API for LLM scripting
3. **Phase 7: CLI Tool** - Build out `debug_command` with all subcommands
4. **Error Handling** - Standardize error responses with codes and suggestions
5. **Documentation** - Add OpenAPI spec and usage examples

## Technical Notes

- Game starts **paused** by default - use `/v1/step` to advance
- Screenshots saved to `/tmp/claude/` directory
- Input overrides persist until reset
- Frame counter tracks actual game frames (not wall time)
- macOS Retina displays: viewport size auto-detected for correct screenshots

## Known Architectural Issues

### Command Integration Complexity

Currently, adding new debug commands (like pathfinding test) requires deep knowledge of the codebase internals and manual integration at multiple layers. This is exemplified by the `/v1/pathfinding-test` HTTP endpoint which cannot easily call the mission's pathfinding test functionality.

**Root Cause**: Input handling is scattered across runtime-specific implementations (desktop P key, VR controller buttons) rather than centralized in the core game logic.

**What Makes This So Challenging**:
1. **Module Privacy**: The `Mission` struct is in a private module, making downcast access complex
2. **Trait Boundaries**: Debug runtime uses `debug_scene()` trait which doesn't expose mission-specific methods
3. **Multiple Abstraction Layers**: Commands flow through Game → Scene → Mission → MissionCore requiring knowledge of each layer
4. **Runtime-Specific Logic**: Desktop runtime handles P key directly in GLFW event loop, bypassing the command system
5. **Effect System Mismatch**: The command/effect system isn't designed for external (HTTP) command injection

**Current Workaround Attempts Failed Because**:
- Direct mission access requires unsafe downcasting through trait objects
- The effect system expects commands to originate from within the game loop
- No clean API exists for external systems to trigger gameplay commands

**Proposed Solution**:
- **Keybinding System**: Move input handling from runtimes → core with a configurable keybinding system
- **Unified Commands**: Desktop P key and HTTP `/v1/pathfinding-test` would both trigger the same core command
- **Runtime Agnostic**: VR runtime could optionally bind pathfinding test to controller buttons
- **Clean Debug API**: Expose mission commands through a well-defined interface

This would make adding debug features trivial: define the command once in core, then optionally bind it to keys/HTTP endpoints as needed.

### Look-At Rotation Not Applied to Camera

**Status**: Bug - look-at calculates rotation but doesn't affect rendered view

The `/v1/player/look-at` endpoint correctly calculates the target rotation and stores it in `stored_input_context.head.rotation`, but the camera orientation in the rendered view doesn't change.

**Symptoms**:
- Endpoint returns success with correct `new_head_rotation` quaternion
- Before/after screenshots show identical views
- Position-based operations (teleport) work correctly

**Root Cause**: The render pipeline uses internal cached head rotation state rather than reading from the `InputContext` passed during `update()`. The `stored_input_context.head.rotation` is passed to `game.update()`, but the scene's render function uses its own stored `self.head_rotation` which may not be updated from the input context.

**Investigation Areas**:
1. Check how `MissionCore::render()` determines camera orientation
2. Verify `update()` copies `input_context.head.rotation` to internal state
3. Compare with how other scenes (debug_map, cutscene_player) handle head rotation

**Workaround**: Use teleport to position player facing the desired direction, or investigate the render pipeline to find where camera rotation is determined.

### Entity Detail Endpoint Returns Wrong Entity

**Status**: Bug - entity IDs are off by one

The `/v1/entities/:id` endpoint returns the wrong entity - consistently off by +1 from the requested ID.

**Symptoms**:
```bash
curl /v1/entities/149  # Returns entity 150 (Wedge Wall Light)
curl /v1/entities/150  # Returns entity 151 (New Tripwire)
curl /v1/entities/547  # Returns entity 548 (New Tripwire)
```

**Workaround**: Use the `/v1/entities?filter=*name*` endpoint which returns correct entity IDs and positions. The list endpoint works correctly; only the detail endpoint has this issue.

**Investigation Areas**:
1. Check the `get_entity_detail` handler's ID parsing
2. Verify entity lookup logic in `DebuggableScene::get_entity_detail()`
