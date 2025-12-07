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
| Screenshot capture (`/v1/screenshot`)           | ✅     |
| macOS Retina scaling fix                        | ✅     |
| Input state read (`/v1/control/input`)          | ✅     |
| Input state write (`/v1/control/input` POST)    | ✅     |
| Multi-level testing (earth.mis, medsci2.mis)    | ✅     |

### Phase 5: Game Commands 🔴 NOT STARTED

| Task                                          | Status         |
| --------------------------------------------- | -------------- |
| Game command endpoint (`/v1/control/command`) | ❌ Placeholder |
| Spawn command                                 | ❌             |
| Save/load commands                            | ❌             |
| Level transition                              | ❌             |
| God mode, noclip                              | ❌             |

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
GET  /v1/health           - Health check
GET  /v1/info             - Game state snapshot
POST /v1/step             - Step simulation (frames or duration)
POST /v1/shutdown         - Graceful shutdown
GET  /v1/entities         - List entities (with ?limit=N&filter=pattern)
GET  /v1/entities/{id}    - Entity details
GET  /v1/player/position  - Player position
POST /v1/player/teleport  - Teleport player
POST /v1/physics/raycast  - Physics raycast
GET  /v1/physics/bodies   - List physics bodies
GET  /v1/physics/bodies/{id} - Physics body details
GET  /v1/control/input    - Get input state
POST /v1/control/input    - Set input channel
POST /v1/control/command  - Execute game command (placeholder)
POST /v1/screenshot       - Capture screenshot
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

1. **[Keybinding System Refactor](keybinding-system.md)** - Prerequisite for Phase 5
   - Centralizes input handling in shock2vr
   - Enables `/v1/input/action` endpoint to trigger any game action
   - Eliminates `Command` trait (direct `InputAction` → `Effect` mapping)
2. **Phase 5: Game Commands** - Implement spawn, save/load, level transition (after keybinding refactor)
3. **Phase 6: TypeScript SDK** - Playwright-style API for LLM scripting
4. **Phase 7: CLI Tool** - Build out `debug_command` with all subcommands
5. **Error Handling** - Standardize error responses with codes and suggestions
6. **Documentation** - Add OpenAPI spec and usage examples

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

**Proposed Solution**: See **[Keybinding System Refactor](keybinding-system.md)**

The keybinding system will:
- Define `InputAction` enum in shock2vr (e.g., `PathfindingTestCycle`)
- Map actions directly to `Effect` variants (eliminating `Command` trait)
- Allow runtimes to map their inputs to these shared actions
- Enable debug runtime to trigger any action via HTTP: `POST /v1/input/action {"action": "PathfindingTestCycle"}`

This makes adding debug features trivial: define the action once in the enum, add the effect mapping, done.
