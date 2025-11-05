# Debug Runtime

Currently, it's difficult for LLMs to actually test the running game and iterate on changes. This project plans to add commands such that an LLM could actually exercise various states of the game and test changes - giving it the ability to "play" shock2vr remotely.

## Architecture

This will add a new runtime - the `debug_runtime`. This will be very similar to the `desktop_runtime` - except it will be fully remote controlled via a localhost-only HTTP server. Commands will be submitted by a CLI tool `debug_command`.

The `debug_command` will `POST` JSON requests to the debug runtime's HTTP endpoint.

## Security Considerations

- **Localhost Only**: The HTTP server will bind to `127.0.0.1:8080` exclusively
- **No Authentication**: Since it's localhost-only, no auth is needed
- **JSON API**: Structured request/response format prevents injection attacks
- **Command Validation**: All commands are validated before execution

## Project Structure

- `tools/debug_command` - CLI tool for sending commands to debug runtime
- `runtimes/debug_runtime` - HTTP-controlled game runtime based on desktop_runtime

### Integration with Existing Systems

The debug runtime will:
- Share core code with `desktop_runtime` (same Scene trait, input handling patterns)
- Follow existing cargo alias pattern: `cargo debug` → `cargo run -p debug_runtime --`
- Use the existing Command trait and Effect system for all game interactions
- Support all experimental features (`--experimental teleport,gui`)
- The application should still render a window, so that it is easy for humans to visually inspect

## Examples

### Starting the Debug Runtime
```bash
# Start with default settings
cargo run -p debug_runtime -- -m=medsci1.mis --debug-physics --port=8080

# Start with experimental features
cargo run -p debug_runtime -- -m=medsci1.mis --experimental teleport,gui --port=8080
```

### CLI Usage Examples
```bash
# Get current game state
cargo run -p debug_command -- info

# Step simulation forward
cargo run -p debug_command -- adv 10
cargo run -p debug_command -- adv 30s

# Take a screenshot
cargo run -p debug_command -- ss debug_test.png

# Perform raycast
cargo run -p debug_command -- rc 0 0 0 10 0 0 world,entity

# List entities near player
cargo run -p debug_command -- ls 20

# Inspect specific entity
cargo run -p debug_command -- ent 442

# Execute game commands
cargo run -p debug_command -- cmd spawn pistol
cargo run -p debug_command -- cmd save debug_state
```

### Direct HTTP API Usage
```bash
# Using RESTful endpoints
curl -X GET http://127.0.0.1:8080/v1/state/info

curl -X POST http://127.0.0.1:8080/v1/control/advance \
  -H "Content-Type: application/json" \
  -d '{"frames": 10}'

curl -X POST http://127.0.0.1:8080/v1/physics/raycast \
  -H "Content-Type: application/json" \
  -d '{"start": [0,0,0], "end": [10,0,0], "collision_groups": ["world", "entity"]}'

# Using unified command interface
curl -X POST http://127.0.0.1:8080/v1/commands/unified \
  -H "Content-Type: application/json" \
  -d '{"command": "info"}'

curl -X POST http://127.0.0.1:8080/v1/commands/unified \
  -H "Content-Type: application/json" \
  -d '{"command": "rc 0 0 0 10 0 0 world,entity"}'
```

### Batch Operations
```bash
# CLI chaining (executed sequentially)
cargo run -p debug_command -- adv 5 && \
cargo run -p debug_command -- rc 0 0 0 10 0 0 && \
cargo run -p debug_command -- info

# HTTP batch (executed atomically)
curl -X POST http://127.0.0.1:8080/v1/commands/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"endpoint": "/control/advance", "data": {"frames": 5}},
      {"endpoint": "/physics/raycast", "data": {"start": [0,0,0], "end": [10,0,0]}},
      {"unified": "info"}
    ]
  }'
```

## HTTP API Specification

This API combines RESTful structure with ergonomic command-line compatibility. The server provides both RESTful endpoints for programmatic access and a unified command interface for CLI tools.

### Base URL
`http://127.0.0.1:8080/v1`

### RESTful Endpoints

All endpoints follow REST conventions with appropriate HTTP verbs and resource-based URLs:

| Method | Endpoint | Description | CLI Command |
|--------|----------|-------------|-------------|
| `GET` | `/state/info` | Get current game state | `debug_command info` |
| `POST` | `/control/advance` | Step simulation | `debug_command adv 10` |
| `POST` | `/control/screenshot` | Capture screenshot | `debug_command ss [file]` |
| `POST` | `/control/input` | Set input state | `debug_command input trigger 1.0` |
| `POST` | `/control/move` | Teleport player/entity | `debug_command move 10 2 15` |
| `POST` | `/physics/raycast` | Perform raycast | `debug_command rc 0 0 0 10 0 0` |
| `GET` | `/entities` | List entities | `debug_command ls 20` |
| `GET` | `/entities/{id}` | Get entity details | `debug_command ent 442` |
| `POST` | `/commands/game` | Execute game command | `debug_command cmd spawn pistol` |
| `POST` | `/commands/batch` | Execute multiple commands | CLI supports chaining |
| `POST` | `/commands/unified` | Unified command interface | All CLI commands route here |

### Unified Command Interface

For maximum CLI ergonomics, all commands can be sent through a single endpoint that parses natural command syntax:

#### POST `/commands/unified`
Accepts commands in the same format as the CLI tool, providing a bridge between human-friendly syntax and RESTful structure.

**Request Format:**
```json
{
  "command": "info"
}
```

**Alternative formats:**
```json
{
  "command": "adv 10"
}
```

```json
{
  "command": "rc 0 0 0 10 0 0 world,entity"
}
```

This endpoint internally routes to the appropriate RESTful endpoint, providing the best of both worlds.

### Standard Response Format

All endpoints return a consistent response structure:

```json
{
  "success": true,
  "data": { /* endpoint-specific response */ },
  "error": null,
  "timestamp": "2024-01-01T12:00:00Z",
  "frame_index": 1247,
  "execution_time_ms": 0.5
}
```

### RESTful Endpoint Details

#### GET `/state/info` - Game State
Returns comprehensive game state information.

**Response:**
```json
{
  "success": true,
  "data": {
    "elapsed_time": 45.23,
    "frame_number": 2711,
    "player": {
      "entity_id": 1234,
      "position": [10.5, 2.0, 15.3],
      "rotation": [0.0, 0.707, 0.0, 0.707],
      "camera_offset": [0.0, 1.6, 0.0],
      "camera_rotation": [0.0, 0.707, 0.0, 0.707]
    },
    "mission": "medsci1.mis",
    "entity_count": 1247,
    "debug_features": ["physics", "ai_debug"],
    "inputs": {
      "head_rotation": [0.0, 0.707, 0.0, 0.707],
      "hands": {
        "left": {
          "position": [9.5, 2.5, 15.0],
          "rotation": [0.0, 0.0, 0.0, 1.0],
          "thumbstick": [0.0, 0.0],
          "trigger": 0.0,
          "squeeze": 0.0,
          "a": 0.0
        },
        "right": { /* same structure */ }
      }
    }
  }
}
```

#### POST `/control/advance` - Step Simulation
Advances game simulation by frames or time.

**Request:**
```json
{
  "frames": 10
}
```

**Alternative:**
```json
{
  "duration": "30s"
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "frames_advanced": 10,
    "time_advanced": 0.166,
    "new_frame_index": 1257,
    "new_total_time": 45.396
  }
}
```

#### POST `/control/screenshot` - Capture Screenshot
Captures the current rendered frame.

**Request:**
```json
{
  "filename": "debug_test.png"
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "filename": "debug_test.png",
    "full_path": "/path/to/Data/debug/screenshots/debug_test.png",
    "resolution": [1920, 1080],
    "size_bytes": 245760
  }
}
```

#### POST `/physics/raycast` - Perform Raycast
Executes a physics raycast with full collision group support.

**Request:**
```json
{
  "start": [10.5, 2.0, 15.3],
  "end": [10.5, 2.0, 25.3],
  "collision_groups": ["world", "entity", "selectable"],
  "max_distance": 100.0
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "hit": true,
    "hit_point": [10.5, 2.0, 18.7],
    "hit_normal": [0.0, 1.0, 0.0],
    "distance": 3.4,
    "entity_id": 442,
    "entity_name": "New Tripwire",
    "collision_group": "entity",
    "is_sensor": false
  }
}
```

#### GET `/entities?limit=20&filter=*Door*` - List Entities
Returns entities sorted by distance from player.

**Query Parameters:**
- `limit` (optional): Maximum entities to return
- `filter` (optional): Name pattern filter with wildcards

**Response:**
```json
{
  "success": true,
  "data": {
    "entities": [
      {
        "id": 442,
        "name": "New Tripwire",
        "template_id": -1718,
        "position": [10.5, 2.0, 18.7],
        "distance": 3.4,
        "script_count": 2,
        "link_count": 5,
        "template": {
          "name": "Tripwire",
          "short_name": "TRIPWIRE"
        }
      }
    ],
    "total_count": 1247,
    "player_position": [10.5, 2.0, 15.3]
  }
}
```

#### POST `/commands/game` - Execute Game Command
Executes built-in game commands using the existing Command trait.

**Request:**
```json
{
  "command": "spawn",
  "args": {
    "template": "pistol",
    "position": [10.0, 2.0, 15.0]
  }
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "command_executed": "SpawnItemCommand",
    "entity_id": 1248,
    "template_id": -17,
    "position": [10.0, 2.0, 15.0]
  }
}
```

### CLI Integration

The `debug_command` CLI tool provides ergonomic access to all endpoints:

**Command Mapping:**
```bash
debug_command info                    # GET /state/info
debug_command adv 10                  # POST /control/advance {"frames": 10}
debug_command adv 30s                 # POST /control/advance {"duration": "30s"}
debug_command ss screenshot.png       # POST /control/screenshot {"filename": "screenshot.png"}
debug_command rc 0 0 0 10 0 0        # POST /physics/raycast {...}
debug_command ls 20                   # GET /entities?limit=20
debug_command ent 442                 # GET /entities/442
debug_command cmd spawn pistol        # POST /commands/game {...}
```

**CLI Options:**
- `--raw` - Output raw JSON (machine-readable)
- `--pretty` - Pretty-print JSON with colors (default)
- `--host <url>` - Override default host (env: `SHOCK_DEBUG_HOST`)
- `--timeout <seconds>` - Request timeout (default: 30s)

### Error Handling

**Standard Error Response:**
```json
{
  "success": false,
  "data": null,
  "error": {
    "code": "INVALID_COMMAND",
    "message": "Unknown command: 'invalid'",
    "details": "Available commands: spawn, save, load, level",
    "suggestions": ["spawn pistol", "save debug_state"]
  },
  "timestamp": "2024-01-01T12:00:00Z"
}
```

**Error Codes:**
- `INVALID_ENDPOINT` - Unknown REST endpoint
- `INVALID_COMMAND` - Unknown unified command
- `INVALID_ARGS` - Wrong arguments for command
- `EXECUTION_ERROR` - Command failed during execution
- `GAME_NOT_READY` - Game not initialized or crashed
- `RAYCAST_FAILED` - Physics raycast encountered error
- `ENTITY_NOT_FOUND` - Referenced entity does not exist

### Batch Operations

#### POST `/commands/batch` - Execute Multiple Commands
Executes multiple commands atomically with rollback on failure.

**Request:**
```json
{
  "commands": [
    {"endpoint": "/control/advance", "data": {"frames": 5}},
    {"endpoint": "/physics/raycast", "data": {"start": [0,0,0], "end": [10,0,0]}},
    {"unified": "info"}
  ]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "results": [
      {"success": true, "data": {"frames_advanced": 5}},
      {"success": true, "data": {"hit": false}},
      {"success": true, "data": {"elapsed_time": 45.23}}
    ],
    "total_execution_time_ms": 2.3
  }
}
```

This API design provides:
1. **RESTful structure** for programmatic access and API clarity
2. **Unified command interface** for CLI ergonomics and LLM simplicity
3. **Consistent responses** across all endpoints
4. **Comprehensive error handling** with helpful suggestions
5. **Batch operations** for atomic multi-command execution
6. **CLI integration** that maps naturally to REST endpoints

## Commands

### Core Information Commands

#### `info` - Current Game State
Returns comprehensive game context.

**Response:**
```json
{
  "elapsed_time": 45.23,
  "frame_number": 2711,
  "player_position": [10.5, 2.0, 15.3],
  "player_rotation": [0.0, 0.707, 0.0, 0.707],
  "camera_position": [10.5, 3.6, 15.3],
  "camera_rotation": [0.0, 0.707, 0.0, 0.707],
  "current_mission": "medsci1.mis",
  "entity_count": 1247,
  "debug_features": ["physics", "ai_debug"]
}
```

#### `ss [filename.png]` - Screenshot
Saves a screenshot of the current rendered frame.

**Arguments:**
- `filename` (optional): Output filename, defaults to `screenshot_TIMESTAMP.png`

**Response:**
```json
{
  "filename": "debug_screenshot_20240101_120000.png",
  "resolution": [1920, 1080],
  "size_bytes": 245760
}
```

### Time and Frame Control

#### `adv [count|time]` - Advance Simulation
Advances the game simulation by frames or time.

**Usage:**
- `adv` - Advance single frame
- `adv 10` - Advance 10 frames
- `adv 30s` - Advance 30 seconds
- `adv 500ms` - Advance 500 milliseconds

**Implementation:** Uses the existing game update loop with controlled time steps.

### Raycast Commands

Based on the physics system analysis, these commands use the existing `ray_cast2()` function with full collision group support.

#### `rc <start_x> <start_y> <start_z> <dest_x> <dest_y> <dest_z> [collision_groups]` - Raycast
Performs a raycast from start point to destination point.

**Arguments:**
- `start_x, start_y, start_z`: Start position in world coordinates
- `dest_x, dest_y, dest_z`: End position (calculates direction and distance)
- `collision_groups` (optional): Comma-separated list of groups to test against
  - Available groups: `world`, `entity`, `selectable`, `player`, `ui`, `hitbox`, `raycast`, `all`
  - Default: `world,entity,selectable`

**Response:**
```json
{
  "hit": true,
  "hit_point": [10.5, 2.0, 18.7],
  "hit_normal": [0.0, 1.0, 0.0],
  "distance": 3.4,
  "entity_id": 442,
  "entity_name": "New Tripwire",
  "collision_group": "entity",
  "is_sensor": false
}
```

**Debug Visualization:** Always draws a debug line (green=miss, red=hit) for 2 seconds.

#### `rcf <entity_id|entity_name> <distance> [collision_groups]` - Raycast From Entity
Performs a raycast from an entity's position in its forward direction.

**Arguments:**
- `entity_id|entity_name`: Source entity (e.g., `442` or `"Security Camera"`)
- `distance`: Maximum raycast distance
- `collision_groups` (optional): Same as `rc` command

**Use Cases:**
- Test AI line-of-sight: `rcf "Security Camera" 50 world`
- Check weapon targeting: `rcf 123 25 entity,hitbox`

### Input Control

Based on the InputContext analysis, the debug runtime can control all input channels remotely.

#### `input` - Read Current Input State
Returns the current InputContext state.

**Response:**
```json
{
  "head": {
    "rotation": [0.0, 0.707, 0.0, 0.707]
  },
  "left_hand": {
    "position": [9.5, 2.5, 15.0],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "thumbstick": [0.0, 0.0],
    "trigger_value": 0.0,
    "squeeze_value": 0.0,
    "a_value": 0.0
  },
  "right_hand": {
    "position": [11.5, 2.5, 15.0],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "thumbstick": [0.2, 0.8],
    "trigger_value": 0.3,
    "squeeze_value": 0.0,
    "a_value": 0.0
  }
}
```

#### `input <channel> <value>` - Set Input State
Overrides specific input channels with provided values.

**Channels:**
- `head.rotation` - Head orientation (quaternion: `[x, y, z, w]`)
- `left_hand.position` - Left hand position (`[x, y, z]`)
- `left_hand.rotation` - Left hand rotation (quaternion)
- `left_hand.thumbstick` - Left thumbstick (`[x, y]` range: -1.0 to 1.0)
- `left_hand.trigger_value` - Left trigger (0.0 to 1.0)
- `left_hand.squeeze_value` - Left squeeze (0.0 to 1.0)
- `left_hand.a_value` - Left A button (0.0 to 1.0)
- `right_hand.*` - Same channels for right hand

**Examples:**
- `input right_hand.trigger_value 1.0` - Full trigger press
- `input left_hand.thumbstick [0.5, -0.8]` - Thumbstick input
- `input head.rotation [0.0, 0.0, 0.0, 1.0]` - Reset head rotation

**Input Persistence:** Input overrides persist until explicitly reset or `input reset` is called.

### Movement Commands

#### `move <x> <y> <z>` - Teleport Player
Teleports the player to the specified world coordinates using `Effect::SetPlayerPosition`.

**Arguments:**
- `x, y, z`: World coordinates to teleport to

**Safety:** Validates coordinates are within mission bounds to prevent crashes.

### Entity Management

#### `ls [count] [filter]` - List Entities
Lists entities sorted by distance from player.

**Arguments:**
- `count` (optional): Maximum entities to return (default: 20)
- `filter` (optional): Name pattern filter (supports wildcards)

**Response:**
```json
{
  "entities": [
    {
      "id": 442,
      "name": "New Tripwire",
      "template_id": -1718,
      "position": [10.5, 2.0, 18.7],
      "distance": 3.4,
      "script_count": 2,
      "link_count": 5
    }
  ],
  "total_count": 1247,
  "player_position": [10.5, 2.0, 15.3]
}
```

#### `ent <id>` - Inspect Entity
Returns detailed entity information including properties, links, and inheritance.

**Response:**
```json
{
  "entity_id": 442,
  "name": "New Tripwire",
  "template_id": -1718,
  "position": [10.5, 2.0, 18.7],
  "rotation": [0.0, 0.0, 0.0, 1.0],
  "inheritance_chain": ["Tripwire", "PhysicalObject", "Object"],
  "properties": [
    {"name": "Position", "value": "[10.5, 2.0, 18.7]"},
    {"name": "Scripts", "value": "Tripwire, TrapTrigger"}
  ],
  "outgoing_links": [
    {"type": "SwitchLink", "target_id": 445, "target_name": "Sound Trap"}
  ],
  "incoming_links": [
    {"type": "Contains", "source_id": 1, "source_name": "MedSci1"}
  ]
}
```

### Global Commands

Based on the command system analysis, these integrate with the existing Command trait infrastructure.

#### `cmd <command_name> [args...]` - Execute Global Command
Executes built-in game commands using the existing command system.

**Available Commands:**
- `spawn <template_id|name> [x] [y] [z]` - Spawn entity (uses `SpawnItemCommand`)
  - Example: `cmd spawn pistol` (spawns pistol template -17 in front of player)
  - Example: `cmd spawn -22 10 2 15` (spawns laser at coordinates)
- `save [filename]` - Quick save (uses `SaveCommand`)
  - Example: `cmd save debug_state`
- `load [filename]` - Quick load (uses `LoadCommand`)
  - Example: `cmd load debug_state`
- `level <mission_name>` - Transition level (uses `TransitionLevelCommand`)
  - Example: `cmd level medsci2.mis`

**Template Reference:**
- Pistol: `-17`
- Laser: `-22`
- Wrench: `-928`
- Assault flash: `-2653`
- Vent parts: `-1998`, `-1999`, `-2000`

**Extended Commands (New Debug Commands):**
- `god` - Toggle invincibility (uses `AdjustHitPoints` effect)
- `give_xp <amount>` - Award experience (uses `AwardXP` effect)
- `noclip` - Toggle collision (disables physics for player)
- `kill <entity_id>` - Destroy entity (uses `SlayEntity` effect)
- `quest <bit_name> <value>` - Set quest bit (uses `SetQuestBit` effect)

## Use Cases

### Debug Camera Logic
```bash
# Teleport in front of security camera
debug_command move 8.0 3.0 12.0
debug_command adv 5  # Let camera detect player
debug_command ent 445  # Inspect camera state
debug_command rcf 445 50 world  # Test camera line-of-sight
```

### Debug Physics Systems
```bash
# Test player falling through floor
debug_command move 10.0 50.0 15.0  # Teleport high up
debug_command cmd noclip  # Disable collision
debug_command adv 60  # Let player fall
debug_command info  # Check if player fell through
```

### Debug AI Behavior
```bash
# Test enemy awareness
debug_command cmd spawn midwife 5.0 2.0 10.0  # Spawn enemy
debug_command move 15.0 2.0 10.0  # Position player in sight
debug_command adv 30  # Let AI update
debug_command rc 5.0 2.0 10.0 15.0 2.0 10.0 world  # Test line-of-sight
```

### Debug Trigger Systems
```bash
# Test tripwire activation
debug_command ent 442  # Inspect tripwire
debug_command move 10.5 2.0 18.0  # Step on tripwire
debug_command adv 5  # Let trigger activate
debug_command ent 445  # Check if sound trap activated
```

## Implementation Details

### Command Processing Architecture
1. **HTTP Server**: Async HTTP server using `tokio` and `warp`
2. **Command Queue**: Thread-safe command queue between HTTP and game threads
3. **Game Integration**: Commands converted to existing `Box<dyn Command>` objects
4. **Response Handling**: Commands return structured data via custom response system

### Debug Runtime Initialization
```rust
// In debug_runtime main.rs
let game_options = GameOptions {
    debug_draw: true,
    experimental_features: vec!["teleport", "gui"],
    remote_control: true,
};

let http_server = DebugHttpServer::new("127.0.0.1:8080");
let command_queue = Arc<Mutex<Vec<Box<dyn Command>>>>::new();

// Main game loop processes both HTTP commands and game updates
```

### Error Recovery
- **Game Crashes**: HTTP server detects game thread panic and reports status
- **Invalid Commands**: Graceful error responses with suggestions
- **Network Issues**: CLI tool retries with exponential backoff
- **State Corruption**: Automatic save/restore on critical errors

### Performance Considerations
- **Frame Rate**: Debug commands executed between frames to maintain 60fps
- **Memory**: Command history limited to prevent memory leaks
- **Network**: Localhost-only ensures minimal latency
- **Rendering**: Screenshot commands use OpenGL framebuffer capture

## Testing and Validation

### Build Validation
Both runtimes must compile before committing:
```bash
# Test debug runtime
cd runtimes/debug_runtime && cargo build

# Test existing desktop runtime still works
cd ../desktop_runtime && cargo build
```

### Integration Testing
- Unit tests for command parsing and validation
- Integration tests for HTTP API endpoints
- Game state consistency tests after command execution
- Performance benchmarks for command latency

This debug runtime provides a powerful foundation for LLM-driven testing and development while maintaining the existing codebase patterns and performance characteristics.

## Implementation Plan

This implementation plan breaks the debug runtime project into manageable phases, following the incremental development process outlined in `CLAUDE.md`.

### Phase 1: Foundation and Scaffolding (Week 1)

**Goal**: Establish the basic project structure and minimal HTTP server.

#### 1.1 Project Structure Setup
- [x] Add `runtimes/debug_runtime` to `Cargo.toml` workspace members
- [x] Add `tools/debug_command` to `Cargo.toml` workspace members
- [x] Create basic `Cargo.toml` files for both crates with dependencies:
  - `debug_runtime`: `tokio`, `axum`, `serde`, `serde_json`, `cgmath`, `shock2vr`, `engine`
  - `debug_command`: `clap`, `reqwest`, `serde`, `serde_json`, `colored`
- [x] Add cargo aliases: `dbgr` and `dbgc`

**Validation**: Both crates compile with `cargo check` ✅

#### 1.2 Minimal HTTP Server
- [ ] Create `runtimes/debug_runtime/src/main.rs` with basic `axum` server
- [ ] Implement localhost-only binding (`127.0.0.1:8080`)
- [ ] Add basic health check endpoint: `GET /v1/health`
- [ ] Add graceful shutdown handling with `tokio::signal`

**Test**: `curl http://127.0.0.1:8080/v1/health` returns 200 OK

#### 1.3 Game Integration Stub
- [ ] Port game initialization from `desktop_runtime/src/main.rs`
- [ ] Create minimal game loop without HTTP integration
- [ ] Ensure rendering window appears and game loads a mission
- [ ] Add command-line argument parsing (mission, port, debug flags)

**Validation**: `cargo run -p debug_runtime -- -m=medsci1.mis` starts game

#### 1.4 Basic CLI Tool
- [ ] Create `tools/debug_command/src/main.rs` with `clap` argument parsing
- [ ] Implement basic HTTP client using `reqwest`
- [ ] Add `debug_command health` subcommand to test connectivity
- [ ] Add `--host` and `--raw` CLI options

**Test**: `debug_command health` connects to running debug runtime

### Phase 2: Core Command Infrastructure (Week 2)

**Goal**: Implement the command processing architecture and basic state queries.

#### 2.1 Command Processing Architecture
- [ ] Define `RuntimeCommand` enum in `debug_runtime/src/commands.rs`:
  ```rust
  enum RuntimeCommand {
      GetInfo(oneshot::Sender<FrameSnapshot>),
      Step(StepSpec, oneshot::Sender<StepResult>),
      // ... other commands
  }
  ```
- [ ] Implement `mpsc` channel between HTTP handlers and game loop
- [ ] Add command queue processing in main game loop
- [ ] Create `FrameSnapshot` struct with basic game state

#### 2.2 State Query Implementation
- [ ] Implement `GET /v1/state/info` endpoint
- [ ] Create `FrameSnapshot` with player position, frame count, elapsed time
- [ ] Add `debug_command info` CLI subcommand
- [ ] Implement standard response format with error handling

**Test**: `debug_command info` returns current game state JSON

#### 2.3 Frame Stepping
- [ ] Implement `POST /v1/control/advance` endpoint
- [ ] Add frame-based stepping: `{"frames": 10}`
- [ ] Add time-based stepping with `humantime` parsing: `{"duration": "30s"}`
- [ ] Implement `debug_command adv` with both syntaxes
- [ ] Ensure deterministic frame advancement (no implicit time progression)

**Test**: `debug_command adv 10` advances exactly 10 frames

#### 2.4 Error Handling System
- [ ] Define error codes enum (`INVALID_COMMAND`, `EXECUTION_ERROR`, etc.)
- [ ] Implement standard error response format
- [ ] Add helpful error messages and suggestions
- [ ] Test error cases (invalid commands, wrong arguments)

**Validation**: All endpoints return consistent error format

### Phase 3: DebuggableScene Trait and Entity System (Week 3)

**Goal**: Implement entity inspection and game world queries.

#### 3.1 DebuggableScene Trait Design
- [ ] Define `DebuggableScene` trait in `shock2vr/src/game_scene.rs`:
  ```rust
  pub trait DebuggableScene {
      fn list_entities(&self, limit: Option<usize>) -> Vec<DebugEntitySummary>;
      fn entity_detail(&self, id: EntityId) -> Option<DebugEntityDetail>;
      fn raycast(&self, start: Point3<f32>, dir: Vector3<f32>, max_toi: f32, mask: RaycastMask) -> Option<DebugRayHit>;
      fn teleport_player(&mut self, position: Vector3<f32>) -> Result<(), String>;
  }
  ```
- [ ] Update `GameScene` trait to optionally implement `DebuggableScene`
- [ ] Modify `Game` struct to expose `debug_scene()` accessor

#### 3.2 Mission DebuggableScene Implementation
- [ ] Implement `DebuggableScene` for `Mission` by delegating to `MissionCore`
- [ ] Implement `list_entities()` using existing ECS views (sort by distance to player)
- [ ] Implement `entity_detail()` with entity properties, links, inheritance chain
- [ ] Use existing entity analysis patterns from `dark_query` tool

**Test**: Entity listing matches `dark_query` output format

#### 3.3 Entity REST Endpoints
- [ ] Implement `GET /v1/entities?limit=20&filter=*Door*`
- [ ] Implement `GET /v1/entities/{id}` for detailed entity inspection
- [ ] Add `debug_command ls` and `debug_command ent` CLI subcommands
- [ ] Include template metadata and inheritance information

**Test**: `debug_command ls 10` and `debug_command ent 442` return expected data

#### 3.4 Player Movement
- [ ] Implement `teleport_player()` using `Effect::SetPlayerPosition`
- [ ] Add `POST /v1/control/move` endpoint
- [ ] Implement `debug_command move x y z` CLI command
- [ ] Add coordinate validation and bounds checking

**Test**: `debug_command move 10 2 15` teleports player to coordinates

### Phase 4: Physics and Input Systems (Week 4)

**Goal**: Implement raycasting and input control capabilities.

#### 4.1 Physics Raycast Integration
- [ ] Implement `raycast()` method using existing `MissionCore::physics.ray_cast2()`
- [ ] Add collision group parsing (world, entity, selectable, etc.)
- [ ] Implement `POST /v1/physics/raycast` endpoint
- [ ] Add debug line visualization using `Effect::DrawDebugLines`
- [ ] Implement `debug_command rc` and `debug_command rcf` CLI commands

**Test**: Raycast commands match physics behavior from virtual hand system

#### 4.2 Input Control System
- [ ] Extend `DebuggableScene` trait with input control methods
- [ ] Implement input state override system based on existing `InputContext`
- [ ] Add `POST /v1/control/input` endpoint for setting input channels
- [ ] Implement `debug_command input` CLI command with channel syntax
- [ ] Support head rotation, hand positions, trigger values, thumbstick input

**Test**: Input overrides affect game behavior (e.g., trigger press activates objects)

#### 4.3 Screenshot Capture
- [ ] Extend `engine::Engine` trait with `capture_framebuffer()` method
- [ ] Implement OpenGL framebuffer capture in `engine/src/gl_engine.rs`
- [ ] Add PNG encoding using `image` crate
- [ ] Implement `POST /v1/control/screenshot` endpoint
- [ ] Add `debug_command ss` CLI command with optional filename

**Test**: Screenshots saved to `Data/debug/screenshots/` directory

### Phase 5: Game Commands and Unified Interface (Week 5)

**Goal**: Implement game command execution and unified command interface.

#### 5.1 Game Command Integration
- [ ] Implement `POST /v1/commands/game` endpoint using existing Command trait
- [ ] Support existing commands: `SpawnItemCommand`, `SaveCommand`, `LoadCommand`
- [ ] Add debug-specific commands: god mode, noclip, give XP
- [ ] Implement `debug_command cmd` CLI subcommand
- [ ] Create command parser for natural syntax (e.g., "spawn pistol")

**Test**: `debug_command cmd spawn pistol` creates entity in game world

#### 5.2 Unified Command Interface
- [ ] Implement `POST /v1/commands/unified` endpoint
- [ ] Create command parser that routes to appropriate REST endpoints
- [ ] Support all CLI command syntax through unified interface
- [ ] Add command suggestion system for typos/invalid commands

**Test**: Unified commands produce identical results to REST endpoints

#### 5.3 Batch Operations
- [ ] Implement `POST /v1/commands/batch` endpoint
- [ ] Add atomic command execution with rollback on failure
- [ ] Support mixing REST endpoints and unified commands
- [ ] Add transaction-like semantics for consistency

**Test**: Batch operations execute atomically or fail completely

### Phase 6: Polish and Testing (Week 6)

**Goal**: Add comprehensive testing, documentation, and production readiness.

#### 6.1 Comprehensive Testing
- [ ] Add unit tests for command parsing and validation
- [ ] Add integration tests for HTTP endpoints using `axum-test`
- [ ] Add CLI integration tests with local server fixture
- [ ] Test error conditions and edge cases
- [ ] Add performance benchmarks for command latency

**Validation**: `cargo test -p debug_runtime` and `cargo test -p debug_command` pass

#### 6.2 Documentation and Examples
- [ ] Add comprehensive README for `debug_runtime` with setup instructions
- [ ] Add CLI help text and usage examples for `debug_command`
- [ ] Create example shell scripts for common debugging workflows
- [ ] Document API endpoints with OpenAPI/Swagger spec
- [ ] Add troubleshooting guide for common issues

#### 6.3 Production Readiness
- [ ] Add proper logging with `tracing` and configurable levels
- [ ] Implement graceful error recovery from game crashes
- [ ] Add command history and replay functionality
- [ ] Optimize performance for high-frequency command execution
- [ ] Add telemetry and metrics collection

#### 6.4 Integration with Existing Tools
- [ ] Add cargo alias: `cargo debug` → `cargo run -p debug_runtime --`
- [ ] Ensure compatibility with existing experimental features
- [ ] Test integration with `shodan` automation system
- [ ] Validate build compatibility with both desktop and oculus runtimes

**Validation**: Both runtimes compile successfully after changes

### Implementation Strategy

#### Development Workflow
1. **Small incremental changes** - Each task should be completable in 2-4 hours
2. **Test each increment** - Validate functionality before moving to next task
3. **Build validation** - Ensure both `desktop_runtime` and `oculus_runtime` still compile
4. **Documentation** - Update `CLAUDE.md` with any new patterns discovered

#### Risk Mitigation
- **Game Loop Integration**: Start with simple command queue, evolve to sophisticated async model
- **OpenGL Context**: Reuse existing GLFW window setup from `desktop_runtime`
- **Entity System Complexity**: Leverage existing `dark_query` patterns for entity analysis
- **Performance**: Profile command latency early, optimize hot paths

#### Success Criteria
- [ ] LLM can successfully debug game issues through HTTP API
- [ ] CLI tool provides ergonomic human debugging experience
- [ ] No performance regression compared to `desktop_runtime`
- [ ] All existing tests continue to pass
- [ ] Comprehensive test coverage for new functionality

### Future Extensions (Post-MVP)

#### Advanced Features
- [ ] WebSocket streaming for real-time state updates
- [ ] Deterministic replay system with command recording
- [ ] Rich physics queries (sweeps, overlap tests)
- [ ] Mission scripting integration and custom script execution
- [ ] Entity relationship visualization and graph queries

#### CI/CD Integration
- [ ] Headless testing mode for CI environments
- [ ] Automated regression testing using debug runtime
- [ ] Integration with `shodan` for continuous monitoring
- [ ] Performance benchmarking and alerting

#### Developer Experience
- [ ] Visual debugger web interface
- [ ] Command autocomplete and syntax highlighting
- [ ] Interactive entity inspector with 3D visualization
- [ ] Real-time log streaming and filtering

This implementation plan provides a clear roadmap for building the debug runtime while maintaining the existing codebase quality and following established development patterns.